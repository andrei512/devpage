//! External links: whether each address a page links to outside devpage
//! answers. `devpage check` asks each address once (twice if it can't be
//! reached); the preview asks in the background and shows what it has heard,
//! announcing on the change bus when that changes.
//!
//! How an address is asked is the [`Ask`] port, and where the preview's
//! background asking runs is the [`Background`] port: the `devpage` binary
//! asks the network over HTTP and runs on its runtime; tests answer from a
//! table.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::future::BoxFuture;

use crate::changes::{Change, Publish};

use crate::problem::Problem;

/// One external link: the page it is on, its line, and the address.
pub type Link = (String, Option<usize>, String);

/// What asking an external address once found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// A status below 400, following redirects.
    Fine,
    /// A definite answer of 400 or above: the link is broken.
    Broken(String),
    /// No answer in time, or no connection: maybe the network, maybe the site.
    Unreachable(String),
}

/// Asks an address once, giving it `wait` to answer.
pub trait Ask: Send + Sync {
    fn ask<'a>(&'a self, address: &'a str, wait: Duration) -> BoxFuture<'a, Outcome>;
}

/// Where the preview's background asking runs, and how it waits between
/// tries.
pub trait Background: Send + Sync {
    /// Runs `task` in the background; `false` when there is nowhere to run
    /// it — a unit test rendering a page outside a runtime.
    fn spawn(&self, task: BoxFuture<'static, ()>) -> bool;
    /// Waits `pause`.
    fn sleep(&self, pause: Duration) -> BoxFuture<'static, ()>;
}

/// How long an address gets to answer, and how the preview retries one that
/// didn't.
#[derive(Debug, Clone, Copy)]
pub struct Waits {
    pub first: Duration,
    /// `devpage check`'s second, longer try.
    pub retry: Duration,
    /// The preview's first pause before asking again; it doubles twice.
    pub retry_after: Duration,
}

impl Default for Waits {
    fn default() -> Self {
        Waits {
            first: Duration::from_secs(10),
            retry: Duration::from_secs(30),
            retry_after: Duration::from_secs(30),
        }
    }
}

/// The preview reports an address that can't be reached only after this many
/// failures in a row; one slow answer is the network, not a broken link.
const FAILURES_BEFORE_A_PROBLEM: u32 = 3;

/// What the preview knows about each external address.
enum Answer {
    Pending,
    Fine,
    Broken(String),
    /// Not reached this many times in a row; still being retried.
    Unreachable(u32, String),
}

/// The external-link checker: one per devpage process, shared by every page.
pub struct External {
    ask: Arc<dyn Ask>,
    background: Arc<dyn Background>,
    waits: Waits,
    /// Every address the preview has asked about, for as long as it lives.
    answers: Mutex<HashMap<String, Answer>>,
    /// Where the preview announces a change in what it would show, so open
    /// pages fetch themselves again.
    changes: Option<Arc<dyn Publish>>,
}

impl External {
    pub fn new(
        ask: Arc<dyn Ask>,
        background: Arc<dyn Background>,
        waits: Waits,
        changes: Option<Arc<dyn Publish>>,
    ) -> Arc<Self> {
        Arc::new(External {
            ask,
            background,
            waits,
            answers: Mutex::default(),
            changes,
        })
    }

    /// Checks each address once, however many pages link to it, and reports
    /// a problem for every link to a broken one — for `devpage check`.
    pub async fn check(&self, links: Vec<Link>) -> Vec<Problem> {
        let mut by_address: HashMap<String, Vec<(String, Option<usize>)>> = HashMap::new();
        for (file, line, address) in links {
            by_address.entry(address).or_default().push((file, line));
        }
        let checks = by_address.into_iter().map(|(address, links)| async move {
            let Some(why) = self.broken(&address).await else {
                return Vec::new();
            };
            links
                .into_iter()
                .map(|(file, line)| Problem {
                    file,
                    line,
                    message: format!("{address} — {why}"),
                    target: Some(address.clone()),
                })
                .collect()
        });
        futures::future::join_all(checks)
            .await
            .into_iter()
            .flatten()
            .collect()
    }

    /// Why an address is broken, or `None` when it answers. One that can't be
    /// reached is asked once more, with longer to answer, before it counts.
    async fn broken(&self, address: &str) -> Option<String> {
        match self.ask.ask(address, self.waits.first).await {
            Outcome::Fine => None,
            Outcome::Broken(why) => Some(why),
            Outcome::Unreachable(_) => match self.ask.ask(address, self.waits.retry).await {
                Outcome::Fine => None,
                Outcome::Broken(why) | Outcome::Unreachable(why) => Some(why),
            },
        }
    }

    /// The problems the preview can show now for these links: a definite
    /// error, or an address not reached three times in a row. An address it
    /// hasn't asked about is asked in the background — outside a runtime (a
    /// unit test rendering a page) nothing is.
    pub fn preview(self: &Arc<Self>, links: Vec<Link>) -> Vec<Problem> {
        let mut problems = Vec::new();
        let mut answers = self
            .answers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for (file, line, address) in links {
            match answers.get(&address) {
                Some(Answer::Broken(why)) => problems.push(Problem {
                    file,
                    line,
                    message: format!("{address} — {why}"),
                    target: Some(address),
                }),
                Some(Answer::Unreachable(failures, why))
                    if *failures >= FAILURES_BEFORE_A_PROBLEM =>
                {
                    problems.push(Problem {
                        file,
                        line,
                        message: format!(
                            "{address} — {why}, {FAILURES_BEFORE_A_PROBLEM} times in a row"
                        ),
                        target: Some(address),
                    })
                }
                Some(Answer::Pending | Answer::Fine | Answer::Unreachable(..)) => {}
                None => {
                    // The answers stay locked until `Pending` is in, so the
                    // task can't record an answer before it.
                    if self
                        .background
                        .spawn(Box::pin(Arc::clone(self).watch(address.clone())))
                    {
                        answers.insert(address, Answer::Pending);
                    }
                }
            }
        }
        problems
    }

    /// Asks an address in the background until it gives a definite answer,
    /// retrying one that can't be reached after a growing pause, and asks
    /// open pages to fetch themselves again whenever what they would show
    /// changes.
    async fn watch(self: Arc<Self>, address: String) {
        let mut failures = 0;
        loop {
            let outcome = self.ask.ask(&address, self.waits.first).await;
            let shown_before = failures >= FAILURES_BEFORE_A_PROBLEM;
            let (answer, shown_now, done) = match outcome {
                Outcome::Fine => (Answer::Fine, false, true),
                Outcome::Broken(why) => (Answer::Broken(why), true, true),
                Outcome::Unreachable(why) => {
                    failures += 1;
                    let shown = failures >= FAILURES_BEFORE_A_PROBLEM;
                    (Answer::Unreachable(failures, why), shown, false)
                }
            };
            self.answers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(address.clone(), answer);
            // A problem appeared or cleared: open pages show the difference.
            if shown_now != shown_before
                && let Some(changes) = &self.changes
            {
                // Any page on the bus may link to the address.
                changes.publish(Change::Refresh);
            }
            if done {
                return;
            }
            // 30 s, then 60 s, then every 120 s while it stays out of reach.
            let pause = self.waits.retry_after * (1 << (failures - 1).min(2));
            self.background.sleep(pause).await;
        }
    }

    /// How many times in a row the preview has failed to reach `address`.
    #[cfg(any(test, feature = "testing"))]
    pub fn failures(&self, address: &str) -> u32 {
        match self.answers.lock().unwrap().get(address) {
            Some(Answer::Unreachable(failures, _)) => *failures,
            _ => 0,
        }
    }
}

/// External checkers for tests.
#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::*;

    /// Answers every address fine.
    struct Fine;

    impl Ask for Fine {
        fn ask<'a>(&'a self, _: &'a str, _: Duration) -> BoxFuture<'a, Outcome> {
            Box::pin(async { Outcome::Fine })
        }
    }

    /// Runs nothing in the background.
    struct Nowhere;

    impl Background for Nowhere {
        fn spawn(&self, _: BoxFuture<'static, ()>) -> bool {
            false
        }

        fn sleep(&self, _: Duration) -> BoxFuture<'static, ()> {
            Box::pin(async {})
        }
    }

    /// A checker that finds every address fine and whose preview asks none:
    /// for tests about everything but external links.
    pub fn offline() -> Arc<External> {
        External::new(Arc::new(Fine), Arc::new(Nowhere), Waits::default(), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Answers from a script per address, one outcome per ask, repeating the
    /// last; and counts the asks.
    #[derive(Default)]
    struct Table {
        script: HashMap<String, Vec<Outcome>>,
        asked: Mutex<HashMap<String, usize>>,
    }

    impl Table {
        fn with(mut self, address: &str, outcomes: &[Outcome]) -> Self {
            self.script.insert(address.into(), outcomes.to_vec());
            self
        }

        fn asked(&self, address: &str) -> usize {
            self.asked
                .lock()
                .unwrap()
                .get(address)
                .copied()
                .unwrap_or(0)
        }
    }

    impl Ask for Table {
        fn ask<'a>(&'a self, address: &'a str, _wait: Duration) -> BoxFuture<'a, Outcome> {
            let mut asked = self.asked.lock().unwrap();
            let n = asked.entry(address.into()).or_default();
            let script = &self.script[address];
            let outcome = script[(*n).min(script.len() - 1)].clone();
            *n += 1;
            Box::pin(async move { outcome })
        }
    }

    fn quick() -> Waits {
        Waits {
            first: Duration::from_millis(1),
            retry: Duration::from_millis(1),
            retry_after: Duration::from_millis(1),
        }
    }

    fn link(address: &str) -> Link {
        ("README.md".into(), Some(3), address.into())
    }

    /// Runs on the test's tokio runtime, as `devpage` runs on its own.
    struct Tokio;

    impl Background for Tokio {
        fn spawn(&self, task: BoxFuture<'static, ()>) -> bool {
            let Ok(runtime) = tokio::runtime::Handle::try_current() else {
                return false;
            };
            runtime.spawn(task);
            true
        }

        fn sleep(&self, pause: Duration) -> BoxFuture<'static, ()> {
            Box::pin(tokio::time::sleep(pause))
        }
    }

    /// Hears what the preview publishes.
    struct Heard(tokio::sync::mpsc::UnboundedSender<Change>);

    impl Publish for Heard {
        fn publish(&self, change: Change) {
            let _ = self.0.send(change);
        }
    }

    fn down() -> Outcome {
        Outcome::Unreachable("no connection".into())
    }

    #[tokio::test]
    async fn check_asks_an_unreachable_address_once_more_and_a_broken_one_once() {
        let table = Arc::new(
            Table::default()
                .with("slow", &[down(), Outcome::Fine])
                .with("drop", &[down()])
                .with("gone", &[Outcome::Broken("404 Not Found".into())]),
        );
        let external = External::new(table.clone(), Arc::new(Tokio), quick(), None);
        let found: Vec<String> = external
            .check(vec![link("slow"), link("drop"), link("gone"), link("gone")])
            .await
            .iter()
            .map(ToString::to_string)
            .collect();
        let mut found = found;
        found.sort();
        assert_eq!(
            found,
            vec![
                "README.md:3: drop — no connection",
                "README.md:3: gone — 404 Not Found",
                "README.md:3: gone — 404 Not Found",
            ]
        );
        assert_eq!(
            (
                table.asked("slow"),
                table.asked("drop"),
                table.asked("gone")
            ),
            (2, 2, 1)
        );
    }

    #[tokio::test]
    async fn the_preview_shows_an_unreachable_address_only_after_three_failures() {
        let table = Arc::new(Table::default().with("flaky", &[down()]));
        let (changes, mut heard) = tokio::sync::mpsc::unbounded_channel();
        let external = External::new(
            table,
            Arc::new(Tokio),
            quick(),
            Some(Arc::new(Heard(changes))),
        );
        assert!(external.preview(vec![link("flaky")]).is_empty());
        // Announced once, when the third failure makes it a problem.
        assert_eq!(heard.recv().await, Some(Change::Refresh));
        assert!(external.failures("flaky") >= FAILURES_BEFORE_A_PROBLEM);
        assert_eq!(
            external
                .preview(vec![link("flaky")])
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            vec!["README.md:3: flaky — no connection, 3 times in a row"]
        );
    }

    #[test]
    fn outside_a_runtime_the_preview_asks_nothing() {
        let table = Arc::new(Table::default().with("x", &[Outcome::Fine]));
        let external = External::new(table.clone(), Arc::new(Tokio), quick(), None);
        assert!(external.preview(vec![link("x")]).is_empty());
        assert_eq!(table.asked("x"), 0);
    }
}

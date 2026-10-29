//! Load, open: each request goes at its scheduled time whether or not the
//! ones before it have answered, and its latency counts from that time — so
//! a server that stalls is charged for every request it kept waiting rather
//! than sent fewer of them. And pages left open: each listening on its
//! document's live-update stream, as a reader's page does.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use futures::StreamExt;
use tokio::sync::watch;
use tokio::time::Instant;

/// The answers to one kind of request: how long each took from its
/// scheduled time, sorted, and how many failed.
#[derive(Default)]
pub struct Latencies {
    pub sorted: Vec<Duration>,
    pub errors: usize,
    pub wall: Duration,
}

impl Latencies {
    pub fn of(mut results: Vec<(Duration, bool)>, wall: Duration) -> Self {
        let errors = results.iter().filter(|(_, ok)| !ok).count();
        results.sort();
        Latencies {
            sorted: results.into_iter().map(|(took, _)| took).collect(),
            errors,
            wall,
        }
    }

    pub fn at(&self, quantile: f64) -> Duration {
        if self.sorted.is_empty() {
            return Duration::ZERO;
        }
        let at =
            ((self.sorted.len() as f64 * quantile).ceil() as usize).clamp(1, self.sorted.len());
        self.sorted[at - 1]
    }

    pub fn error_ratio(&self) -> f64 {
        self.errors as f64 / self.sorted.len().max(1) as f64
    }

    pub fn per_second(&self) -> f64 {
        self.sorted.len() as f64 / self.wall.as_secs_f64().max(1e-9)
    }
}

/// `count` requests at `rate` a second, request `i` made by `request(i)` and
/// filed under the class it names; each is timed from its scheduled time.
/// The answers, by class.
pub async fn open<F, Fut>(
    rate: f64,
    count: usize,
    request: F,
) -> std::collections::BTreeMap<&'static str, Latencies>
where
    F: Fn(usize) -> (&'static str, Fut),
    Fut: Future<Output = bool> + Send + 'static,
{
    let gap = Duration::from_secs_f64(1.0 / rate.max(1e-9));
    let start = Instant::now();
    let mut tasks = Vec::with_capacity(count);
    for i in 0..count {
        let scheduled = start + gap.mul_f64(i as f64);
        tokio::time::sleep_until(scheduled).await;
        let (class, answer) = request(i);
        tasks.push(tokio::spawn(async move {
            let ok = answer.await;
            (class, scheduled.elapsed(), ok)
        }));
    }
    let mut by_class: std::collections::BTreeMap<&'static str, Vec<(Duration, bool)>> =
        Default::default();
    for task in tasks {
        if let Ok((class, took, ok)) = task.await {
            by_class.entry(class).or_default().push((took, ok));
        }
    }
    let wall = start.elapsed();
    by_class
        .into_iter()
        .map(|(class, results)| (class, Latencies::of(results, wall)))
        .collect()
}

/// A GET, answered with a success and its whole body read.
pub async fn get(client: reqwest::Client, url: String) -> bool {
    match client.get(url).send().await {
        Ok(answer) => answer.status().is_success() && answer.bytes().await.is_ok(),
        Err(_) => false,
    }
}

/// Pages held open, each listening to a live-update stream.
pub struct Listeners {
    /// How many are connected and still open.
    pub open: Arc<AtomicUsize>,
    /// Each one's count of messages heard, bumped as each arrives.
    pub heard: Vec<watch::Receiver<usize>>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Listeners {
    /// Opens a stream on each of `urls` and waits until each has answered
    /// or failed, at most `within`.
    pub async fn open(client: &reqwest::Client, urls: Vec<String>, within: Duration) -> Self {
        let open = Arc::new(AtomicUsize::new(0));
        let settled = Arc::new(AtomicUsize::new(0));
        let mut heard = Vec::new();
        let mut tasks = Vec::new();
        let total = urls.len();
        for url in urls {
            let (tell, listen) = watch::channel(0usize);
            heard.push(listen);
            let (client, open, settled) = (client.clone(), open.clone(), settled.clone());
            tasks.push(tokio::spawn(async move {
                let answer = client
                    .get(url)
                    .header("accept", "text/event-stream")
                    .send()
                    .await;
                settled.fetch_add(1, Ordering::SeqCst);
                let Ok(answer) = answer else { return };
                if !answer.status().is_success() {
                    return;
                }
                open.fetch_add(1, Ordering::SeqCst);
                let mut body = answer.bytes_stream();
                while let Some(Ok(chunk)) = body.next().await {
                    let messages = chunk
                        .split(|b| *b == b'\n')
                        .filter(|line| line.starts_with(b"data:"))
                        .count();
                    if messages > 0 {
                        tell.send_modify(|n| *n += messages);
                    }
                }
                open.fetch_sub(1, Ordering::SeqCst);
            }));
        }
        let deadline = Instant::now() + within;
        while settled.load(Ordering::SeqCst) < total && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Listeners { open, heard, tasks }
    }

    pub fn open_now(&self) -> usize {
        self.open.load(Ordering::SeqCst)
    }
}

impl Drop for Listeners {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A server that answers one connection at a time, each after `stall`.
    async fn serial(stall: Duration) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let mut buffer = [0u8; 4096];
                let _ = socket.read(&mut buffer).await;
                tokio::time::sleep(stall).await;
                let _ = socket
                    .write_all(
                        b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok",
                    )
                    .await;
            }
        });
        format!("http://{address}/")
    }

    fn client() -> reqwest::Client {
        reqwest::Client::builder()
            .pool_max_idle_per_host(0)
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn a_server_that_stalls_is_charged_its_stall() {
        // 20 answers a second asked for 100: the requests queue, and each
        // counts the wait from its scheduled time. A load that waited for
        // each answer before the next would report 50 ms.
        let url = serial(Duration::from_millis(50)).await;
        let client = client();
        let answers = open(100.0, 40, |_| ("page", get(client.clone(), url.clone()))).await;
        let page = &answers["page"];
        assert_eq!(page.sorted.len(), 40);
        assert_eq!(page.errors, 0);
        assert!(
            page.at(0.99) > Duration::from_millis(1200),
            "{:?}",
            page.at(0.99)
        );
        assert!(
            page.at(0.5) > Duration::from_millis(400),
            "{:?}",
            page.at(0.5)
        );
    }

    #[tokio::test]
    async fn a_server_that_keeps_up_is_charged_only_its_answer() {
        let url = serial(Duration::ZERO).await;
        let client = client();
        let answers = open(50.0, 25, |_| ("page", get(client.clone(), url.clone()))).await;
        assert!(answers["page"].at(0.99) < Duration::from_millis(100));
    }

    #[test]
    fn a_quantile_is_the_slowest_of_its_share() {
        let l = Latencies::of(
            (1..=100)
                .map(|i| (Duration::from_millis(i), i != 7))
                .collect(),
            Duration::from_secs(1),
        );
        assert_eq!(l.at(0.5), Duration::from_millis(50));
        assert_eq!(l.at(0.99), Duration::from_millis(99));
        assert_eq!(l.errors, 1);
    }
}

//! The diagram engine: every Mermaid fence drawn to SVG on the server, by the
//! vendored pele bundle running in QuickJS.
//!
//! A QuickJS runtime can't move between threads, so one thread of its own —
//! the diagram thread — owns it, with pele loaded once, and draws what it is
//! sent over a channel. In front of it, a cache keeps each drawing by the
//! diagram's source. Each drawing gets a time limit, enforced by QuickJS's
//! interrupt handler, and the runtime a memory limit; a runtime that hit one
//! is dropped, and the next diagram gets a fresh one.

use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::panic::AssertUnwindSafe;
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use rquickjs::{Context, Ctx, Function, Runtime};

/// The vendored bundle: one script that defines `pele`.
const PELE: &str = include_str!("../assets/vendor/pele.js");

/// What pele needs of the browser beyond plain JavaScript: a `URL` — it
/// lowercases an `http:` or `https:` `click` target's scheme and host with
/// one, and leaves the target out of the drawing when `new URL` throws — then
/// the one function the engine calls.
const PRELUDE: &str = r#"
globalThis.URL = class URL {
  constructor(address) {
    const m = /^([a-zA-Z][a-zA-Z0-9+.-]*:)\/\/(?:([^@/?#]*)@)?([^:/?#]+)(:\d*)?([^]*)$/.exec(String(address));
    if (!m) throw new TypeError("Invalid URL: " + address);
    this.protocol = m[1];
    this.userinfo = m[2];
    this.hostname = m[3];
    this.port = m[4] === ":" ? "" : (m[4] ?? "");
    this.rest = (m[5].startsWith("/") ? m[5] : "/" + m[5]).replace(/ /g, "%20");
  }
  toString() {
    const user = this.userinfo === undefined ? "" : this.userinfo + "@";
    return this.protocol + "//" + user + this.hostname + this.port + this.rest;
  }
};
"#;

const DRAW: &str = "globalThis.__devpageDraw = (source) => pele.render(source).svg;";

/// How long one drawing may take, and how much memory the runtime may hold.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    pub time: Duration,
    pub memory: usize,
}

impl Limits {
    pub const SERVER: Limits = Limits {
        time: Duration::from_secs(2),
        memory: 128 << 20,
    };
}

/// How many drawings the cache keeps; the oldest goes first.
const CACHE_SIZE: usize = 1024;

/// What drawing a diagram gave: its SVG, or why there is none.
pub type Drawn = Result<Arc<str>, String>;

/// Why a diagram has no drawing.
#[derive(Debug)]
enum Failure {
    /// pele's error: the same source always fails the same way.
    Pele(String),
    /// A limit, or the engine itself: worth trying again later.
    Engine(String),
}

/// One page's diagrams, sent to the diagram thread at once.
struct Job {
    sources: Vec<String>,
    reply: mpsc::Sender<Vec<Result<String, Failure>>>,
}

/// The diagram thread, and the cache in front of it.
pub(crate) struct Engine {
    jobs: mpsc::Sender<Job>,
    cache: Mutex<Cache>,
    /// How many diagrams were sent to the thread: those the cache didn't have.
    #[cfg(test)]
    sent: std::sync::atomic::AtomicUsize,
}

#[derive(Default)]
struct Cache {
    drawn: HashMap<String, Drawn>,
    order: VecDeque<String>,
}

impl Cache {
    fn insert(&mut self, source: String, drawn: Drawn) {
        if self.drawn.insert(source.clone(), drawn).is_none() {
            self.order.push_back(source);
        }
        while self.order.len() > CACHE_SIZE {
            if let Some(oldest) = self.order.pop_front() {
                self.drawn.remove(&oldest);
            }
        }
    }
}

/// devpage's engine, started on first use.
static ENGINE: LazyLock<Engine> = LazyLock::new(|| Engine::start(Limits::SERVER));

/// Starts the diagram thread, which loads pele in the background, so the
/// first page with a diagram doesn't wait for it.
pub fn start() {
    let engine = LazyLock::force(&ENGINE);
    // A job with nothing to draw makes the thread load pele now.
    let (reply, _) = mpsc::channel();
    let _ = engine.jobs.send(Job {
        sources: Vec::new(),
        reply,
    });
}

/// Draws each diagram, from the cache when it was drawn before.
pub fn draw_all(sources: &[String]) -> Vec<Drawn> {
    ENGINE.draw_all(sources)
}

/// Replaces each Mermaid block of rendered HTML — `<pre lang="mermaid">`, as
/// the highlighter leaves it — with its drawing in a `<div class="diagram">`,
/// or, when it can't be drawn, with the error block: `Diagram error:`, the
/// reason, and the diagram's source.
pub fn draw_fences(html: &str) -> String {
    draw_fences_with(&ENGINE, html)
}

const OPEN: &str = "<pre lang=\"mermaid\"><code>";
const CLOSE: &str = "</code></pre>";

fn draw_fences_with(engine: &Engine, html: &str) -> String {
    if !html.contains(OPEN) {
        return html.to_string();
    }
    // Each block: where it starts and ends, and its source.
    let mut blocks = Vec::new();
    let mut from = 0;
    while let Some(start) = html[from..].find(OPEN).map(|at| from + at) {
        let body = start + OPEN.len();
        let Some(end) = html[body..].find(CLOSE).map(|at| body + at) else {
            break;
        };
        blocks.push((start, end + CLOSE.len(), unescape(&html[body..end])));
        from = end + CLOSE.len();
    }
    let sources: Vec<String> = blocks.iter().map(|(_, _, s)| s.clone()).collect();
    let drawn = engine.draw_all(&sources);
    let mut out = String::with_capacity(html.len());
    let mut from = 0;
    for ((start, end, source), drawn) in blocks.iter().zip(drawn) {
        out.push_str(&html[from..*start]);
        match drawn {
            Ok(svg) => {
                out.push_str("<div class=\"diagram\">");
                out.push_str(&svg);
                out.push_str("</div>");
            }
            Err(why) => out.push_str(&format!(
                "<pre class=\"diagram-error\">Diagram error: {}\n\n{}</pre>",
                escape(&why),
                escape(source)
            )),
        }
        from = *end;
    }
    out.push_str(&html[from..]);
    out
}

/// The text of code the highlighter escaped.
fn unescape(html: &str) -> String {
    html.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

impl Engine {
    /// Starts a diagram thread with these limits.
    pub(crate) fn start(limits: Limits) -> Engine {
        let (jobs, received) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("diagrams".into())
            // QuickJS recurses on the native stack; its own limit, below,
            // stops a deep diagram well before this one runs out.
            .stack_size(16 << 20)
            .spawn(move || serve(limits, received))
            .expect("the diagram thread");
        Engine {
            jobs,
            cache: Mutex::default(),
            #[cfg(test)]
            sent: Default::default(),
        }
    }

    pub(crate) fn draw_all(&self, sources: &[String]) -> Vec<Drawn> {
        let mut drawn: Vec<Option<Drawn>> = {
            let cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
            sources
                .iter()
                .map(|s| cache.drawn.get(s).cloned())
                .collect()
        };
        let missing: Vec<usize> = (0..sources.len()).filter(|&i| drawn[i].is_none()).collect();
        if !missing.is_empty() {
            #[cfg(test)]
            self.sent
                .fetch_add(missing.len(), std::sync::atomic::Ordering::Relaxed);
            let (reply, answer) = mpsc::channel();
            let job = Job {
                sources: missing.iter().map(|&i| sources[i].clone()).collect(),
                reply,
            };
            let answers = self
                .jobs
                .send(job)
                .ok()
                .and_then(|()| answer.recv().ok())
                .unwrap_or_default();
            let mut answers = answers.into_iter();
            let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
            for i in missing {
                drawn[i] = Some(match answers.next() {
                    Some(Ok(svg)) => {
                        let svg: Arc<str> = svg.into();
                        cache.insert(sources[i].clone(), Ok(svg.clone()));
                        Ok(svg)
                    }
                    Some(Err(Failure::Pele(why))) => {
                        cache.insert(sources[i].clone(), Err(why.clone()));
                        Err(why)
                    }
                    Some(Err(Failure::Engine(why))) => Err(why),
                    None => Err("the diagram engine stopped".to_string()),
                });
            }
        }
        drawn
            .into_iter()
            .map(|d| d.unwrap_or_else(|| Err("the diagram engine stopped".to_string())))
            .collect()
    }
}

/// The diagram thread: draws each job's diagrams in order, with one runtime
/// kept from job to job until a limit drops it.
fn serve(limits: Limits, jobs: mpsc::Receiver<Job>) {
    let mut pele: Option<Pele> = None;
    for job in jobs {
        if pele.is_none() {
            // Failing here is failing on the first diagram, below, again.
            pele = std::panic::catch_unwind(|| Pele::load(limits).ok())
                .ok()
                .flatten();
        }
        let mut answers = Vec::with_capacity(job.sources.len());
        for source in &job.sources {
            let drawn = std::panic::catch_unwind(AssertUnwindSafe(|| {
                if pele.is_none() {
                    pele = Some(Pele::load(limits)?);
                }
                let drawn = pele.as_ref().map(|p| p.draw(source));
                drawn.unwrap_or_else(|| Err(Failure::Engine("no runtime".into())))
            }))
            .unwrap_or_else(|_| Err(Failure::Engine("the diagram engine failed".into())));
            // A runtime past a limit — or one that panicked — may be left in
            // any state: the next diagram gets a fresh one.
            if matches!(drawn, Err(Failure::Engine(_))) {
                pele = None;
            }
            answers.push(drawn);
        }
        let _ = job.reply.send(answers);
    }
}

/// A QuickJS runtime with pele loaded.
struct Pele {
    // Dropped before the runtime it lives in.
    context: Context,
    _runtime: Runtime,
    limits: Limits,
    /// When the drawing under way must stop, if one is.
    deadline: Rc<Cell<Option<Instant>>>,
    /// Whether the interrupt handler stopped the last drawing.
    interrupted: Rc<Cell<bool>>,
}

impl Pele {
    fn load(limits: Limits) -> Result<Pele, Failure> {
        let fail = |what: &str, error: rquickjs::Error| {
            Failure::Engine(format!("the diagram engine couldn't {what}: {error}"))
        };
        let runtime = Runtime::new().map_err(|e| fail("start", e))?;
        runtime.set_memory_limit(limits.memory);
        runtime.set_max_stack_size(4 << 20);
        let deadline: Rc<Cell<Option<Instant>>> = Rc::default();
        let interrupted: Rc<Cell<bool>> = Rc::default();
        {
            let (deadline, interrupted) = (deadline.clone(), interrupted.clone());
            runtime.set_interrupt_handler(Some(Box::new(move || {
                let stop = deadline.get().is_some_and(|at| Instant::now() >= at);
                if stop {
                    interrupted.set(true);
                }
                stop
            })));
        }
        let context = Context::full(&runtime).map_err(|e| fail("start", e))?;
        let pele = Pele {
            context,
            _runtime: runtime,
            limits,
            deadline,
            interrupted,
        };
        // Loading is not a drawing: it has no time limit.
        pele.run(None, |ctx| {
            ctx.eval::<(), _>(PRELUDE)?;
            ctx.eval::<(), _>(PELE)?;
            ctx.eval::<(), _>(DRAW)
        })
        .map_err(|failure| match failure {
            Failure::Pele(why) => Failure::Engine(format!("pele didn't load: {why}")),
            engine => engine,
        })?;
        Ok(pele)
    }

    fn draw(&self, source: &str) -> Result<String, Failure> {
        self.run(Some(self.limits.time), |ctx| {
            let draw: Function = ctx.globals().get("__devpageDraw")?;
            draw.call((source,))
        })
    }

    /// Runs `f` in the context, stopped after `time` if it is given, turning
    /// a thrown exception into its message and a limit into the engine's
    /// failure.
    fn run<T>(
        &self,
        time: Option<Duration>,
        f: impl FnOnce(&Ctx<'_>) -> rquickjs::Result<T>,
    ) -> Result<T, Failure> {
        self.interrupted.set(false);
        self.deadline.set(time.map(|time| Instant::now() + time));
        // What was thrown, as its message; `None` when QuickJS had no memory
        // left even for an error, and threw nothing.
        let result = self.context.with(|ctx| {
            f(&ctx).map_err(|error| match error {
                rquickjs::Error::Exception => {
                    let thrown = ctx.catch();
                    if let Some(exception) = thrown.as_exception() {
                        Some(exception.message().unwrap_or_default())
                    } else if let Some(text) = thrown.as_string() {
                        text.to_string().ok()
                    } else if thrown.is_null() || thrown.is_undefined() {
                        None
                    } else {
                        Some(format!("{thrown:?}"))
                    }
                }
                rquickjs::Error::Allocation => None,
                other => Some(other.to_string()),
            })
        });
        self.deadline.set(None);
        result.map_err(|why| match why {
            _ if self.interrupted.get() => Failure::Engine(format!(
                "took longer than {} to draw",
                seconds(self.limits.time)
            )),
            None => Failure::Engine("ran out of memory".into()),
            Some(why) if why == "out of memory" => Failure::Engine("ran out of memory".into()),
            Some(why) => Failure::Pele(why),
        })
    }
}

fn seconds(time: Duration) -> String {
    match time.as_secs() {
        1 if time.subsec_nanos() == 0 => "1 second".into(),
        n if time.subsec_nanos() == 0 => format!("{n} seconds"),
        _ => format!("{} ms", time.as_millis()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    fn engine(time: Duration, memory: usize) -> Engine {
        Engine::start(Limits { time, memory })
    }

    fn one(engine: &Engine, source: &str) -> Drawn {
        engine.draw_all(&[source.to_string()]).remove(0)
    }

    /// A flowchart of `n` nodes in a chain.
    fn chain(n: usize) -> String {
        let mut source = String::from("flowchart TD\n");
        for i in 0..n {
            source.push_str(&format!("  n{i}[Node {i}] --> n{}\n", i + 1));
        }
        source
    }

    #[test]
    fn a_diagram_is_drawn_to_svg_with_its_links() {
        let engine = Engine::start(Limits::SERVER);
        let svg = one(
            &engine,
            "flowchart LR\n  a[A] --> b\n  click a href \"./x.md\" \"Ex\"\n  click b href \"https://Example.COM/x y\"\n",
        )
        .unwrap();
        assert!(
            svg.starts_with(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" class=\"pele pele-flowchart\""
            ),
            "{svg}"
        );
        assert!(
            svg.contains("<a href=\"./x.md\" rel=\"noopener\"><title>Ex</title>"),
            "{svg}"
        );
        // The engine's `URL` is enough for pele to keep an https target.
        assert!(
            svg.contains("<a href=\"https://example.com/x%20y\" rel=\"noopener\">"),
            "{svg}"
        );
        // Colors stay variables, for the page's theme to set.
        assert!(svg.contains("var(--pele-fg,"), "{svg}");
    }

    #[test]
    fn pele_s_error_is_the_diagram_s_and_the_engine_goes_on() {
        let engine = Engine::start(Limits::SERVER);
        let error = one(&engine, "sequenceDiagram\n  A->>B: one; two\n").unwrap_err();
        assert!(error.starts_with("Parse error on line 2:"), "{error}");
        assert_eq!(
            one(&engine, "nonsense").unwrap_err(),
            "No diagram type detected for the given text."
        );
        assert!(one(&engine, "flowchart LR\n  a --> b\n").is_ok());
    }

    #[test]
    fn a_diagram_drawn_before_comes_from_the_cache() {
        let engine = Engine::start(Limits::SERVER);
        let good = "flowchart LR\n  a --> b\n".to_string();
        let bad = "nonsense".to_string();
        let first = engine.draw_all(&[good.clone(), bad.clone()]);
        assert_eq!(engine.sent.load(Ordering::Relaxed), 2);
        let again = engine.draw_all(&[bad.clone(), good.clone(), "pie\n  \"x\": 1\n".into()]);
        // Only the new one was drawn; the others, error included, are kept.
        assert_eq!(engine.sent.load(Ordering::Relaxed), 3);
        assert_eq!(again[0], first[1]);
        assert!(Arc::ptr_eq(
            again[1].as_ref().unwrap(),
            first[0].as_ref().unwrap()
        ));
    }

    #[test]
    fn a_diagram_past_the_time_limit_fails_and_is_tried_again() {
        let engine = engine(Duration::from_millis(1), 128 << 20);
        let big = chain(500);
        assert_eq!(
            one(&engine, &big).unwrap_err(),
            "took longer than 1 ms to draw"
        );
        // Not kept: the next page with it tries again.
        assert_eq!(
            one(&engine, &big).unwrap_err(),
            "took longer than 1 ms to draw"
        );
        assert_eq!(engine.sent.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn a_diagram_past_the_memory_limit_fails_and_the_next_gets_a_fresh_runtime() {
        // Enough to load pele, not to draw 1,600 nodes.
        let engine = engine(Duration::from_secs(10), 10 << 20);
        assert_eq!(one(&engine, &chain(1600)).unwrap_err(), "ran out of memory");
        assert!(one(&engine, "flowchart LR\n  a --> b\n").is_ok());
    }

    #[test]
    fn fences_become_drawings_or_error_blocks() {
        let engine = Engine::start(Limits::SERVER);
        let html = "<p>x</p>\n<pre lang=\"mermaid\"><code>flowchart LR\n  a[&quot;A &amp; B&quot;] --&gt; b\n</code></pre>\n<pre lang=\"mermaid\"><code>nope &lt;b&gt;\n</code></pre>\n<pre><code>plain</code></pre>\n";
        let drawn = draw_fences_with(&engine, html);
        assert!(
            drawn.starts_with("<p>x</p>\n<div class=\"diagram\"><svg"),
            "{drawn}"
        );
        assert!(drawn.contains(">A &amp; B</text>"), "{drawn}");
        assert!(
            drawn.contains("</svg></div>\n<pre class=\"diagram-error\">Diagram error: No diagram type detected for the given text.\n\nnope &lt;b&gt;\n</pre>\n<pre><code>plain</code></pre>\n"),
            "{drawn}"
        );
    }
}

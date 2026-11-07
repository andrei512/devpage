//! TeX math in Markdown, rendered to native MathML by the pinned KaTeX bundle.
//! Comrak owns delimiter parsing, including code and escapes. The same rendered
//! errors reach the page and its check; no browser script or network is needed.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use comrak::nodes::NodeValue;
use comrak::options::Plugins;
use comrak::{Arena, Options, format_html_with_plugins, parse_document};
use rquickjs::{Context, Function, Runtime};

use crate::html::escape;

/// KaTeX 0.16.11's unmodified distribution bundle; its license lives beside it.
const KATEX: &str = include_str!("../assets/vendor/katex.js");
/// A formula may contain at most 16 KiB of TeX before rendering is refused.
const MAX_SOURCE: usize = 16 << 10;
/// Each formula gets one second, including macro expansion and MathML generation.
const TIME: Duration = Duration::from_secs(1);
/// A render has its own macro scope; commands may not fetch resources or add HTML.
const RENDER: &str = r#"
globalThis.__devpageMath = (source, display) => katex.renderToString(source, {
  output: "mathml", displayMode: display, throwOnError: true, strict: "error",
  trust: false, maxExpand: 1000, maxSize: 20, macros: {}
});
"#;

thread_local! {
    /// QuickJS stays on the thread that created it and loads KaTeX once there.
    static ENGINE: RefCell<Option<Engine>> = const { RefCell::new(None) };
}

/// A thread's math runtime, bounded independently of the page's other work.
struct Engine {
    /// The loaded KaTeX context, dropped before its runtime.
    context: Context,
    /// Keeps the runtime alive while the context exists, limited to 64 MiB.
    _runtime: Runtime,
    /// The deadline inspected by QuickJS while it executes JavaScript.
    deadline: Rc<Cell<Option<Instant>>>,
}

impl Engine {
    /// Loads the pinned renderer without exposing filesystem or network APIs.
    fn new() -> Result<Self, String> {
        let runtime = Runtime::new().map_err(|e| e.to_string())?;
        runtime.set_memory_limit(64 << 20);
        runtime.set_max_stack_size(256 << 10);
        let deadline: Rc<Cell<Option<Instant>>> = Rc::default();
        let stop = deadline.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || {
            stop.get().is_some_and(|at| Instant::now() >= at)
        })));
        let context = Context::full(&runtime).map_err(|e| e.to_string())?;
        context
            .with(|ctx| {
                ctx.eval::<(), _>(KATEX)?;
                ctx.eval::<(), _>(RENDER)
            })
            .map_err(|e| format!("math renderer could not load: {e}"))?;
        Ok(Self {
            context,
            _runtime: runtime,
            deadline,
        })
    }

    /// Typesets one formula, retaining its TeX as the MathML accessibility annotation.
    fn render(&self, source: &str, display: bool) -> Result<String, String> {
        self.deadline.set(Some(Instant::now() + TIME));
        let result = self.context.with(|ctx| {
            let run = || -> rquickjs::Result<String> {
                let render: Function = ctx.globals().get("__devpageMath")?;
                render.call((source, display))
            };
            run().map_err(|error| {
                if error.is_exception() {
                    ctx.catch()
                        .as_exception()
                        .and_then(|e| e.message())
                        .unwrap_or_else(|| "math renderer exceeded its resource limit".into())
                } else {
                    error.to_string()
                }
            })
        });
        self.deadline.set(None);
        result
    }
}

/// Typesets a bounded formula; a failed runtime is discarded before the next one.
fn typeset(source: &str, display: bool) -> Result<String, String> {
    if source.len() > MAX_SOURCE {
        return Err("formula exceeds 16 KiB".into());
    }
    ENGINE.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(Engine::new()?);
        }
        let result = slot.as_ref().unwrap().render(source, display);
        if result.is_err() {
            *slot = None;
        }
        result
    })
}

/// Formats Markdown with its native math nodes replaced by MathML or checked
/// error spans. Code spans, fenced code, HTML and escaped delimiters stay literal.
pub(crate) fn markdown(source: &str, options: &Options<'_>, plugins: &Plugins<'_>) -> String {
    let arena = Arena::new();
    let root = parse_document(&arena, source, options);
    for node in root.descendants() {
        let mut data = node.data.borrow_mut();
        if let NodeValue::Math(math) = &data.value {
            let html = match typeset(&math.literal, math.display_math) {
                Ok(html) => html,
                Err(why) => {
                    let message = format!("{}: Math error: {why}", data.sourcepos.start.line);
                    format!(
                        "<span class=\"math-error\" data-math-error=\"{}\">Math error: {} <code>{}</code></span>",
                        escape(&message),
                        escape(&why),
                        escape(&math.literal)
                    )
                }
            };
            data.value = NodeValue::HtmlInline(html);
        }
    }
    let mut out = String::new();
    format_html_with_plugins(root, options, &mut out, plugins)
        .expect("writing Markdown to a string");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formulas_are_mathml_with_tex_preserved_and_no_remote_assets() {
        let html = typeset(r"\frac{1}{2}+\sqrt{x_1^2}", true).unwrap();
        for tag in [
            "<math",
            "<mfrac>",
            "<msqrt>",
            "<msubsup>",
            "display=\"block\"",
            "application/x-tex",
        ] {
            assert!(html.contains(tag), "{html}");
        }
        assert!(!html.contains("src="));
        assert!(!html.contains("<script"));
    }

    #[test]
    fn invalid_and_recursive_formulas_fail_and_next_formula_still_renders() {
        assert!(typeset(r"\unknowncommand{", false).is_err());
        assert!(typeset(r"\def\x{\x}\x", false).is_err());
        assert!(typeset(&"x".repeat(MAX_SOURCE + 1), false).is_err());
        assert!(typeset("x+1", false).unwrap().contains("<math"));
        assert!(
            typeset(r"\href{https://example.com}{x}", false)
                .map_or(true, |html| !html.contains("href="))
        );
    }
}

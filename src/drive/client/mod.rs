//! The CLI's server commands: contexts, publishing, and acting on documents
//! already on a server. Only `publish` reads files; every other command names
//! a document by its id on the current context's server.

pub mod api;
pub mod bundle;
pub mod contexts;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use clap::{Args, Subcommand};

use crate::document::Document;
use api::{Api, Thread};
use contexts::{Context, Home};

/// The commands that talk to a devpage server.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// The servers the CLI talks to.
    Context {
        #[command(subcommand)]
        action: ContextAction,
    },
    /// Publish a document: the first time creates it, after that adds a version.
    Publish {
        /// A document file, a folder holding devpage.yml, a Markdown file, or a design.
        path: PathBuf,
        /// Publish to this document, whatever the CLI remembers.
        #[arg(long, conflicts_with = "new")]
        id: Option<String>,
        /// Always create a new document.
        #[arg(long)]
        new: bool,
        #[command(flatten)]
        remote: Remote,
    },
    /// The documents on the server.
    List {
        #[command(flatten)]
        remote: Remote,
    },
    /// A document's versions.
    Versions {
        id: String,
        #[command(flatten)]
        remote: Remote,
    },
    /// A document's open threads, grouped by the Markdown file they are on.
    Comments(CommentsArgs),
    /// Save the PDF or PNG of a document's latest version, or of one version.
    Download {
        id: String,
        #[arg(long)]
        version: Option<u64>,
        /// Where to write it; `<title>.pdf` or `<title>.png` by default.
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[command(flatten)]
        remote: Remote,
    },
    /// Delete a document — its versions and comments — or one version.
    Delete {
        id: String,
        #[arg(long)]
        version: Option<u64>,
        #[command(flatten)]
        remote: Remote,
    },
}

#[derive(Subcommand, Debug)]
pub enum ContextAction {
    /// Add a context: a name for a server.
    Add {
        name: String,
        server: String,
        /// A command printing a bearer token for the proxy in front of the server.
        #[arg(long)]
        token_command: Option<String>,
    },
    /// Make a context the current one.
    Use { name: String },
    /// The contexts, the current one marked.
    List {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Args, Debug)]
#[command(args_conflicts_with_subcommands = true)]
pub struct CommentsArgs {
    #[command(subcommand)]
    action: Option<CommentAction>,
    id: Option<String>,
    #[command(flatten)]
    remote: Remote,
}

#[derive(Subcommand, Debug)]
pub enum CommentAction {
    /// Reply to a thread.
    Reply {
        id: String,
        thread: String,
        text: String,
        #[command(flatten)]
        remote: Remote,
    },
    /// Resolve a thread: it is deleted.
    Resolve {
        id: String,
        thread: String,
        #[command(flatten)]
        remote: Remote,
    },
}

/// Which server a command talks to, and how it answers.
#[derive(Args, Debug, Default, Clone)]
pub struct Remote {
    /// Use this context instead of the current one (or `DEVPAGE_CONTEXT`).
    #[arg(long)]
    context: Option<String>,
    /// Answer in JSON.
    #[arg(long)]
    json: bool,
}

/// What a command prints, and the code it exits with.
#[derive(Debug, PartialEq)]
pub struct Output {
    pub text: String,
    pub code: i32,
}

impl Output {
    fn ok(text: String) -> Output {
        Output { text, code: 0 }
    }
}

/// Runs `command` with the CLI's files in `home`, reading documents in the
/// served folder `served`; `context` is `DEVPAGE_CONTEXT`, when set.
pub async fn run(
    command: Command,
    home: &Home,
    served: &Path,
    context: Option<String>,
) -> Result<Output> {
    let chosen = |remote: &Remote| remote.context.clone().or_else(|| context.clone());
    match command {
        Command::Context { action } => contexts_command(action, home),
        Command::Publish {
            path,
            id,
            new,
            remote,
        } => publish(home, served, &path, id, new, &chosen(&remote), remote.json).await,
        Command::List { remote } => {
            let (api, _) = connect(home, &chosen(&remote))?;
            let documents = api.list().await?;
            if remote.json {
                return json(&documents);
            }
            let mut text = String::new();
            for d in &documents {
                let link = api.link(&format!("/d/{}", d.id));
                writeln!(
                    text,
                    "{}  {}  {}  v{}  {link}",
                    d.id, d.kind, d.title, d.latest
                )?;
            }
            Ok(Output::ok(text))
        }
        Command::Versions { id, remote } => {
            let (api, _) = connect(home, &chosen(&remote))?;
            let document = api.describe(&id).await?;
            if remote.json {
                return json(&document.versions);
            }
            let mut text = String::new();
            for v in &document.versions {
                writeln!(text, "v{}  {}  {}", v.number, v.published, v.author)?;
            }
            Ok(Output::ok(text))
        }
        Command::Comments(args) => comments(home, args, &context).await,
        Command::Download {
            id,
            version,
            output,
            remote,
        } => {
            let (api, _) = connect(home, &chosen(&remote))?;
            let document = api.describe(&id).await?;
            let extension = match document.kind.as_str() {
                "doc" | "slides" => "pdf",
                "design" => "png",
                kind => bail!("{id}: a {kind} has no download — only docs, slides and designs do"),
            };
            let version = version.unwrap_or(document.latest);
            let bytes = api.download(&id, version).await?;
            let output = output.unwrap_or_else(|| {
                crate::pdf::default_output(&document.title).with_extension(extension)
            });
            std::fs::write(&output, &bytes)
                .with_context(|| format!("writing {}", output.display()))?;
            if remote.json {
                return json(&serde_json::json!({
                    "id": id, "version": version, "file": output, "bytes": bytes.len(),
                }));
            }
            Ok(Output::ok(format!("{}\n", output.display())))
        }
        Command::Delete {
            id,
            version,
            remote,
        } => {
            let (api, _) = connect(home, &chosen(&remote))?;
            api.delete(&id, version).await?;
            if remote.json {
                return json(&serde_json::json!({ "id": id, "version": version, "deleted": true }));
            }
            Ok(Output::ok(match version {
                Some(n) => format!("deleted version {n} of {id}\n"),
                None => format!("deleted {id}\n"),
            }))
        }
    }
}

fn json<T: serde::Serialize>(value: &T) -> Result<Output> {
    Ok(Output::ok(format!(
        "{}\n",
        serde_json::to_string_pretty(value)?
    )))
}

fn connect(home: &Home, context: &Option<String>) -> Result<(Api, String)> {
    let (name, context) = home.context(context.as_deref())?;
    Ok((Api::connect(&context)?, name))
}

fn contexts_command(action: ContextAction, home: &Home) -> Result<Output> {
    let mut config = home.config()?;
    match action {
        ContextAction::Add {
            name,
            server,
            token_command,
        } => {
            if !(server.starts_with("http://") || server.starts_with("https://")) {
                bail!("{server}: a server is an http:// or https:// address");
            }
            config.contexts.insert(
                name.clone(),
                Context {
                    server,
                    token_command,
                },
            );
            home.save_config(&config)?;
            Ok(Output::ok(format!("added context {name}\n")))
        }
        ContextAction::Use { name } => {
            if !config.contexts.contains_key(&name) {
                bail!("no context named {name}");
            }
            config.current_context = Some(name.clone());
            home.save_config(&config)?;
            Ok(Output::ok(format!("using context {name}\n")))
        }
        ContextAction::List { json: as_json } => {
            if as_json {
                return json(&config);
            }
            let mut text = String::new();
            for (name, context) in &config.contexts {
                let mark = if config.current_context.as_deref() == Some(name) {
                    "*"
                } else {
                    " "
                };
                writeln!(text, "{mark} {name}  {}", context.server)?;
            }
            Ok(Output::ok(text))
        }
    }
}

/// Checks the document — external addresses too — zips what the check
/// followed, and publishes it to the document the CLI remembers for this
/// file on this context, or to `id`, or to a new one.
async fn publish(
    home: &Home,
    served: &Path,
    path: &Path,
    id: Option<String>,
    new: bool,
    context: &Option<String>,
    as_json: bool,
) -> Result<Output> {
    let document = Document::load(path, served)?;
    let problems = crate::check::document(&document, Some(&crate::http::external(None))).await;
    if !problems.is_empty() {
        let lines: Vec<String> = problems.iter().map(ToString::to_string).collect();
        let text = if as_json {
            format!(
                "{}\n",
                serde_json::to_string_pretty(&serde_json::json!({ "problems": lines }))?
            )
        } else {
            format!("{}\nnothing published\n", lines.join("\n"))
        };
        return Ok(Output { text, code: 1 });
    }
    let bundle = bundle::bundle(&document)?;
    let (api, name) = connect(home, context)?;
    let file = document.path();
    let mut published: BTreeMap<PathBuf, String> = home.published(&name)?;
    let target = match (id, new) {
        (_, true) => None,
        (Some(id), false) => Some(id),
        (None, false) => published.get(&file).cloned(),
    };
    let answer = api
        .publish(target.as_deref(), bundle.zip, &bundle.entry)
        .await
        .map_err(|error| match &target {
            // A document deleted on the server, or an id from elsewhere.
            Some(id) if error.to_string().contains("not found") => anyhow::anyhow!(
                "{error}: no document {id} on context {name} — publish with --new to create one"
            ),
            _ => error,
        })?;
    published.insert(file, answer.id.clone());
    home.save_published(&name, &published)?;
    let link = api.link(&answer.url);
    if as_json {
        return json(&serde_json::json!({
            "id": answer.id, "version": answer.version, "url": link, "files": bundle.files,
        }));
    }
    Ok(Output::ok(format!(
        "{} — version {}\n{link}\n",
        document.title, answer.version
    )))
}

async fn comments(home: &Home, args: CommentsArgs, context: &Option<String>) -> Result<Output> {
    let chosen = |remote: &Remote| remote.context.clone().or_else(|| context.clone());
    match args.action {
        Some(CommentAction::Reply {
            id,
            thread,
            text,
            remote,
        }) => {
            let (api, _) = connect(home, &chosen(&remote))?;
            api.reply(&id, &thread, &text).await?;
            if remote.json {
                return json(&serde_json::json!({ "id": id, "thread": thread, "replied": true }));
            }
            Ok(Output::ok(format!("replied to {thread}\n")))
        }
        Some(CommentAction::Resolve { id, thread, remote }) => {
            let (api, _) = connect(home, &chosen(&remote))?;
            api.resolve(&id, &thread).await?;
            if remote.json {
                return json(&serde_json::json!({ "id": id, "thread": thread, "resolved": true }));
            }
            Ok(Output::ok(format!("resolved {thread}\n")))
        }
        None => {
            let Some(id) = args.id else {
                bail!("which document? `devpage comments <id>`");
            };
            let (api, _) = connect(home, &chosen(&args.remote))?;
            let threads = api.comments(&id).await?;
            if args.remote.json {
                return json(&threads);
            }
            let latest = api.describe(&id).await?.latest;
            Ok(Output::ok(threads_text(&threads, latest)))
        }
    }
}

/// The threads as text, grouped by page: each thread's id, where it is, its
/// quote in its context, and its messages.
fn threads_text(threads: &[Thread], latest: u64) -> String {
    let mut pages: BTreeMap<&str, Vec<&Thread>> = BTreeMap::new();
    for thread in threads {
        pages.entry(thread.page.as_str()).or_default().push(thread);
    }
    let width = threads
        .iter()
        .flat_map(|t| t.thread.iter().map(|m| m.author.chars().count()))
        .max()
        .unwrap_or(0);
    let mut text = String::new();
    for (page, threads) in pages {
        let _ = writeln!(text, "{}", page.trim_start_matches('/'));
        for thread in threads {
            let place = if thread.detached {
                format!("detached in v{latest}")
            } else {
                format!("on v{}", thread.version)
            };
            let _ = writeln!(
                text,
                "  {}  {place}  \"{}\"",
                thread.id, thread.anchor.quote
            );
            if thread.detached {
                continue;
            }
            let a = &thread.anchor;
            let _ = writeln!(text, "    …{}[{}]{}", a.prefix, a.quote, a.suffix);
            for message in &thread.thread {
                let _ = writeln!(
                    text,
                    "    {:width$}  {}  {}",
                    message.author,
                    clock(&message.at),
                    message.text
                );
            }
        }
    }
    text
}

/// `HH:MM` of a timestamp written as `YYYY-MM-DDTHH:MM…`, or it as written.
fn clock(at: &str) -> &str {
    match at.get(10..11) {
        Some("T") | Some(" ") => at.get(11..16).unwrap_or(at),
        _ => at,
    }
}

#[cfg(test)]
mod tests;

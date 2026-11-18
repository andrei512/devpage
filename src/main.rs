//! devpage: serves the documents of a repository — sites, docs and slides
//! written in Markdown — live in the browser, and prints them to PDF.

// The code is arranged by role. The core — documents, rendering, checking,
// comments, designs — is the `devpage-core` crate, which depends on no I/O
// runtime; the binary plugs adapters into its ports and drives it.

/// Driven adapters: what the core's ports are plugged into.
mod adapters;
/// Driving adapters: what drives the core — the HTTP server, Chrome.
mod drive;

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tokio::net::TcpListener;

/// The first port tried; when it is taken, the next one up, until one is free.
const FIRST_PORT: u16 = 3030;

/// What `devpage --help` ends with: the commands a first run needs.
const EXAMPLES: &str = "\
Examples:
  devpage README.md            Serve a Markdown file; open the address it prints
  devpage docs                 Serve the site, doc or deck in docs/devpage.yml
  devpage check docs --json    Check every link of a document; exit 1 if one is broken
  devpage pdf talk             Print a doc or deck to <title>.pdf
  devpage png app.design       Draw a design to <title>.png, no server needed
  devpage png talk --slide 3   Draw slide 3 of a deck
  devpage checkout.design      Draw a design on a canvas

Exit codes: 0 done, 1 the document has problems (printed), 2 devpage couldn't run.
With --json, a result is JSON on standard output and an error JSON on standard error.

Docs: https://devpage.ai/docs/README.md";

/// devpage's command line: a command, or a document to serve and how to
/// serve it.
#[derive(Parser)]
#[command(version, about, after_help = EXAMPLES)]
struct Cli {
    /// What to do; `None` serves `path`.
    #[command(subcommand)]
    command: Option<Command>,
    /// What to serve: a Markdown file, a design (.design), a folder holding
    /// devpage.yml, or a document file. The current folder by default.
    path: Option<PathBuf>,
    /// The served folder: nothing outside it is served.
    #[command(flatten)]
    root: drive::client::Root,
    /// The address to listen on; any non-loopback address means deployed.
    #[arg(long, default_value = "127.0.0.1")]
    host: IpAddr,
    /// Listen on exactly this port instead of the first free one from 3030.
    #[arg(long)]
    port: Option<u16>,
    /// The header naming a comment's author when deployed.
    #[arg(long, default_value = "X-Forwarded-Email")]
    user_header: String,
    /// Turn comments off: the pages show none, and the comment routes answer 404.
    #[arg(long)]
    no_comments: bool,
}

/// The commands besides serving a document.
#[derive(Subcommand)]
enum Command {
    /// Check every link; exit 1 if one is broken.
    ///
    /// Checks the document and every page it reaches by links, external
    /// addresses too, and prints each problem as `<file>:<line>: <problem>`.
    Check {
        /// The document: a Markdown file, a design, a folder holding
        /// devpage.yml, or a document file. The current folder by default.
        path: Option<PathBuf>,
        /// Answer in JSON: `{"problems": [{"file", "line", "message"}]}`.
        #[arg(long)]
        json: bool,
        /// Check the docs only: leave a site's requirements and its tests out.
        #[arg(long)]
        docs: bool,
        #[command(flatten)]
        root: drive::client::Root,
    },
    /// Print a doc or a deck to PDF, as the browser shows it.
    Pdf {
        /// The doc or deck: a Markdown file, a folder holding devpage.yml, or
        /// a document file.
        path: PathBuf,
        /// Where to write the PDF; `<title>.pdf` by default.
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[command(flatten)]
        root: drive::client::Root,
    },
    /// Draw a design, or one slide of a deck, to a PNG; no server needed.
    ///
    /// Prints the file written. A design with problems is not drawn: they
    /// are printed as `devpage check` prints them, and it exits 1.
    Png {
        /// The design (.design), or the deck: a folder holding its
        /// devpage.yml, or its document file.
        path: PathBuf,
        /// The slide to draw, from 1; a deck needs one.
        #[arg(long)]
        slide: Option<usize>,
        /// The theme a design is drawn in; light by default.
        #[arg(long, value_parser = ["light", "dark"])]
        theme: Option<String>,
        /// Where to write the PNG; `<title>.png`, or `<title> <n>.png` for a
        /// slide, by default.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Answer in JSON: `{"png": "<file>"}`, or `{"problems": […]}`.
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        root: drive::client::Root,
    },
    /// Serve published documents for a team, every version kept.
    ///
    /// Configured only by `DEVPAGE_*` variables, every one listed at
    /// https://devpage.ai/docs/boxes/viewer/share/server/reference/README.md#environment
    Server,
    /// The commands that talk to a devpage server.
    #[command(flatten)]
    Remote(drive::client::Command),
    /// Install the live session: its program, and the bun that runs it.
    ///
    /// Unpacks the program this devpage carries into ~/.devpage/live/program,
    /// downloads devpage's own bun into ~/.devpage/live/bun and installs the
    /// program's packages with it. Run again after updating devpage; when
    /// everything is installed already it changes nothing.
    Setup,
    /// Record a deck to mp4, each slide's notes read aloud by GPT-Live.
    ///
    /// Needs `DEVPAGE_OPENAI_API_KEY`, or `DEVPAGE_AZURE_OPENAI_ENDPOINT` and
    /// `DEVPAGE_AZURE_OPENAI_API_KEY`; `DEVPAGE_VIDEO_MODEL` picks the model.
    Video {
        /// The deck: a folder holding its devpage.yml, or its document file.
        path: PathBuf,
        /// Where to write the video; `<title>.mp4` by default.
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[command(flatten)]
        root: drive::client::Root,
    },
}

impl Command {
    /// Whether the command answers in JSON, and so reports an error in JSON.
    fn json(&self) -> bool {
        match self {
            Command::Check { json, .. } | Command::Png { json, .. } => *json,
            Command::Remote(command) => command.json(),
            Command::Pdf { .. } | Command::Server | Command::Setup | Command::Video { .. } => false,
        }
    }
}

/// devpage's exit codes: 0 done, 1 the document has problems, 2 devpage
/// couldn't run — as clap exits on a command line it can't read.
const PROBLEMS: u8 = 1;
/// The exit code when devpage couldn't run.
const FAILED: u8 = 2;

/// Runs the command line, exiting as [`run`] says, or with [`FAILED`] after
/// printing its error — as JSON when the command answers in JSON.
#[tokio::main]
async fn main() -> ExitCode {
    adapters::extensions::install();
    let cli = Cli::parse();
    let json = cli.command.as_ref().is_some_and(Command::json);
    match run(cli).await {
        Ok(code) => code,
        Err(error) => {
            if json {
                let causes: Vec<String> = error.chain().skip(1).map(ToString::to_string).collect();
                eprintln!(
                    "{}",
                    serde_json::json!({ "error": error.to_string(), "causes": causes })
                );
            } else {
                eprintln!("Error: {error:?}");
            }
            ExitCode::from(FAILED)
        }
    }
}

/// Runs each test suite of a site's requirements, in the document file's
/// folder, and answers what its JUnit report shows is wrong: each failed
/// test, or a command that printed no report. What a command writes to
/// standard error is shown as it comes.
fn tested(document: &devpage_core::document::Document) -> Vec<devpage_core::problem::Problem> {
    use devpage_core::requirements;
    let Some(configured) = &document.requirements else {
        return Vec::new();
    };
    let mut problems = Vec::new();
    let mut cases = Vec::new();
    for suite in &configured.suites {
        let mut command = if cfg!(windows) {
            let mut command = std::process::Command::new("cmd");
            command.arg("/C");
            command
        } else {
            let mut command = std::process::Command::new("sh");
            command.arg("-c");
            command
        };
        let output = command
            .arg(&suite.run)
            .current_dir(&document.base)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::inherit())
            .output();
        let (printed, exit) = match &output {
            Ok(output) => (
                String::from_utf8_lossy(&output.stdout).into_owned(),
                output.status.code(),
            ),
            Err(_) => (String::new(), None),
        };
        problems.extend(requirements::run_problems(document, suite, &printed, exit));
        cases.extend(requirements::junit(&printed));
    }
    // Kept for the `Requirements` widget, which shows each constraint as
    // met or not by this run.
    let checked = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    if let Err(error) =
        requirements::save(document, &requirements::results(document, &cases, checked))
    {
        eprintln!("The run's results were not kept: {error:#}");
    }
    problems
}

/// The problems found, printed one a line or as JSON, and the code to exit
/// with.
fn reported(problems: &[devpage_core::problem::Problem], json: bool) -> ExitCode {
    if json {
        println!("{}", serde_json::json!({ "problems": problems }));
    } else {
        for problem in problems {
            println!("{problem}");
        }
    }
    if problems.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(PROBLEMS)
    }
}

/// Runs `cli`'s command, or serves its document: the composition root, where
/// the core's ports meet their adapters.
async fn run(mut cli: Cli) -> Result<ExitCode> {
    // The composition root's own names, kept to it: every other module names
    // the core's and the binary's modules by their paths.
    use adapters::{changes, external, git, terminal};
    use devpage_core::document::{Document, download_name};
    use devpage_core::folder::Folder;
    use devpage_core::{check, comments, designs, diagrams, requirements};
    use drive::{http, pdf, png, video};
    diagrams::start();
    if let Some(Command::Check {
        path,
        json,
        docs,
        root,
    }) = &cli.command
    {
        let document = Arc::new(Document::load(
            path.as_deref().unwrap_or(std::path::Path::new(".")),
            &root.served()?,
        )?);
        let designs = designs::Designs::new(&document.served);
        let folder = Folder::new(document.clone());
        let update = (!json).then(updates);
        let mut problems = check::document(&folder, &designs, Some(&external::checker(None))).await;
        if !docs {
            problems.extend(requirements::problems(&document));
            problems.extend(tested(&document));
        }
        let code = reported(&problems, *json);
        if let Some(update) = update {
            told(update).await;
        }
        return Ok(code);
    }
    match cli.command.take() {
        Some(Command::Pdf { path, output, root }) => {
            let document = Arc::new(Document::load(&path, &root.served()?)?);
            let output = output.unwrap_or_else(|| download_name(&document.title, "pdf"));
            let update = updates();
            let bytes = pdf::print(document).await?;
            std::fs::write(&output, bytes)
                .with_context(|| format!("writing {}", output.display()))?;
            println!("{}", output.display());
            told(update).await;
            return Ok(ExitCode::SUCCESS);
        }
        Some(Command::Png {
            path,
            slide,
            theme,
            output,
            json,
            root,
        }) => {
            let document = Arc::new(Document::load(&path, &root.served()?)?);
            let output = output.unwrap_or_else(|| match slide {
                Some(n) => download_name(&format!("{} {n}", document.title), "png"),
                None => download_name(&document.title, "png"),
            });
            match png::draw(document.clone(), slide, theme.as_deref()).await? {
                png::Drawn::Png(bytes) => {
                    std::fs::write(&output, bytes)
                        .with_context(|| format!("writing {}", output.display()))?;
                    if json {
                        println!("{}", serde_json::json!({ "png": output }));
                    } else {
                        println!("{}", output.display());
                    }
                    return Ok(ExitCode::SUCCESS);
                }
                png::Drawn::Broken => {
                    let designs = designs::Designs::new(&document.served);
                    let problems = check::document(&Folder::new(document), &designs, None).await;
                    return Ok(reported(&problems, json));
                }
            }
        }
        Some(Command::Video { path, output, root }) => {
            let config = video::narrator::Config::from_env()?;
            let document = Arc::new(Document::load(&path, &root.served()?)?);
            let output = output.unwrap_or_else(|| download_name(&document.title, "mp4"));
            // However the run ends — done, failed or interrupted — the
            // recording future is dropped, and its Chrome with it.
            tokio::select! {
                recorded = video::record(document, &output, &config) => recorded?,
                signal = interrupted() => anyhow::bail!("stopped by {signal}"),
            }
            println!("{}", output.display());
            return Ok(ExitCode::SUCCESS);
        }
        Some(Command::Server) => {
            let config = drive::server_mode::Config::from_env()?;
            let address = SocketAddr::new(config.host, config.port);
            let (app, metrics_router) = drive::server_mode::routers(&config)?;
            let listener = TcpListener::bind(address).await?;
            let metrics =
                TcpListener::bind(SocketAddr::new(config.host, config.metrics_port)).await?;
            println!("devpage server on http://{}", listener.local_addr()?);
            println!("metrics on http://{}/metrics", metrics.local_addr()?);
            tokio::spawn(async move { axum::serve(metrics, metrics_router).await });
            axum::serve(listener, app).await?;
            return Ok(ExitCode::SUCCESS);
        }
        Some(Command::Remote(command)) => {
            let home = drive::client::contexts::Home::from_env()?;
            let context = std::env::var("DEVPAGE_CONTEXT").ok();
            let output = drive::client::run(command, &home, context).await?;
            print!("{}", output.text);
            return Ok(ExitCode::from(output.code));
        }
        Some(Command::Setup) => {
            let home = drive::client::contexts::Home::from_env()?;
            adapters::live::Installed::new(&home.0).setup().await?;
            return Ok(ExitCode::SUCCESS);
        }
        Some(Command::Check { .. }) | None => {}
    }
    let path = cli.path.unwrap_or_else(|| PathBuf::from("."));
    let served = cli.root.served()?;
    let home = drive::client::contexts::Home::from_env()?;
    let installed = adapters::live::Installed::new(&home.0);
    // The live session acts as whoever runs devpage, as every extension's
    // process does: it is on only where devpage answers this machine alone,
    // and only with the key its models need, OpenAI's or Azure's.
    if cli.host.is_loopback() {
        let config = home.config()?;
        if adapters::live::has_provider() {
            match config
                .live_program()
                .and_then(|program| installed.package(program.as_deref()))
            {
                Ok(package) => devpage_core::extensions::serve_live(&served, package),
                Err(error) => eprintln!("No live session: {error:#}"),
            }
        }
        devpage_core::extensions::serve_packages(&served, &home.0, config.extensions);
    }
    let document = Arc::new(Document::load(&path, &served)?);

    if cli.host.is_loopback() {
        devpage_core::extensions::serve_entry(&served, document.path());
    }

    // The process-wide parts: the change bus, the link checker on the
    // network reporting to the terminal, and who writes a comment — the
    // local git name on a loopback address, the user header otherwise — or
    // no one: comments are off.
    let changes = changes::Changes::default();
    let preview = Arc::new(check::Preview::new(
        external::checker(Some(changes.clone())),
        Arc::new(terminal::Terminal),
    ));
    let identity = if cli.no_comments {
        None
    } else if cli.host.is_loopback() {
        Some(comments::Identity::Local(git::user_name(
            document.served.path(),
        )))
    } else {
        Some(comments::Identity::Header(cli.user_header))
    };
    let comments = identity.map(|identity| {
        let threads = comments::Comments::new(
            Arc::new(comments::yaml::Yaml::new(&document.served)),
            Arc::new(changes.clone()),
            document.served.clone(),
        );
        http::Commenting::new(threads, identity)
    });
    let app = http::App::local(document.clone(), changes, preview, comments);
    let _watcher = app.watch()?;
    let shots = app.shots();

    let listener = match cli.port {
        Some(port) => TcpListener::bind(SocketAddr::from((cli.host, port)))
            .await
            .with_context(|| format!("binding {}:{port}", cli.host))?,
        None => bind(cli.host, FIRST_PORT).await?,
    };
    let port = listener.local_addr()?.port();
    let shown = if cli.host.is_loopback() {
        "localhost".to_string()
    } else {
        cli.host.to_string()
    };
    println!("{} → http://{shown}:{port}", document.title);
    // An extension's process acts as whoever runs devpage, so one is started
    // only where devpage answers this machine alone.
    let processes = cli.host.is_loopback().then(|| {
        adapters::processes::Processes::new(
            format!("http://{shown}:{port}"),
            git::user_name(document.served.path()),
            installed.bun(),
        )
    });
    let app = match &processes {
        Some(processes) => app.with_processes(processes.clone()),
        None => app,
    };
    // Served on another address, devpage is deployed: no one there updates it.
    if cli.host.is_loopback() {
        let update = updates();
        tokio::spawn(async move {
            if let Ok(Some(line)) = update.await {
                eprintln!("{line}");
            }
        });
    }
    let router = http::logged(http::router(app));
    // Stopped, the server closes the Chrome that draws its slides' PNGs.
    tokio::select! {
        served = axum::serve(listener, router) => served?,
        _ = interrupted() => {}
    }
    shots.close().await;
    if let Some(processes) = processes {
        processes.stop().await;
    }
    Ok(ExitCode::SUCCESS)
}

/// Starts asking, in the background, whether a newer devpage is out.
fn updates() -> tokio::task::JoinHandle<Option<String>> {
    tokio::spawn(adapters::update::notice())
}

/// Prints the line saying a newer devpage is out, if the asking `updates`
/// started has answered so: a command that is done never waits for it.
async fn told(update: tokio::task::JoinHandle<Option<String>>) {
    if update.is_finished()
        && let Ok(Some(line)) = update.await
    {
        eprintln!("{line}");
    }
}

/// Waits for Ctrl-C, SIGTERM or SIGHUP, and names it.
#[cfg(unix)]
async fn interrupted() -> &'static str {
    use tokio::signal::unix::{SignalKind, signal};
    let (Ok(mut term), Ok(mut hangup)) = (
        signal(SignalKind::terminate()),
        signal(SignalKind::hangup()),
    ) else {
        tokio::signal::ctrl_c().await.ok();
        return "Ctrl-C";
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => "Ctrl-C",
        _ = term.recv() => "SIGTERM",
        _ = hangup.recv() => "SIGHUP",
    }
}

/// Waits for Ctrl-C, Ctrl-Break or the console closing, and names it.
#[cfg(windows)]
async fn interrupted() -> &'static str {
    use tokio::signal::windows::{ctrl_break, ctrl_close};
    let (Ok(mut ctrl_break), Ok(mut close)) = (ctrl_break(), ctrl_close()) else {
        tokio::signal::ctrl_c().await.ok();
        return "Ctrl-C";
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => "Ctrl-C",
        _ = ctrl_break.recv() => "Ctrl-Break",
        _ = close.recv() => "the console closing",
    }
}

/// Binds the first free port at or above `first`.
async fn bind(host: IpAddr, first: u16) -> Result<TcpListener> {
    for port in first..=u16::MAX {
        if let Ok(listener) = TcpListener::bind(SocketAddr::from((host, port))).await {
            return Ok(listener);
        }
    }
    None.context("no free port")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn takes_the_next_port_when_the_first_is_busy() {
        let host = IpAddr::from([127, 0, 0, 1]);
        let busy = bind(host, 41000).await.unwrap();
        let taken = busy.local_addr().unwrap().port();
        let next = bind(host, taken).await.unwrap();
        assert!(next.local_addr().unwrap().port() > taken);
    }
}

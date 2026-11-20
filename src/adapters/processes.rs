//! Extension processes: the long-running program a package names with `run`,
//! started on the first request under its extension's `live/` address,
//! restarted when it has died and stopped when its extension is no longer
//! bound or devpage exits. A process listens on a loopback port devpage
//! picks; only requests carrying the token devpage made for it come from
//! devpage.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use devpage_core::extensions::{self, Live};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::{Child, Command};

/// How long a started process has to accept connections on its port.
const STARTING: Duration = Duration::from_secs(20);
/// How long a process told to stop has before it is killed.
#[cfg(unix)]
const STOPPING: Duration = Duration::from_secs(3);
/// A process that ran this long before dying is started again at once.
const SETTLED: Duration = Duration::from_secs(10);
/// The longest wait before a process that keeps dying is started again.
const LONGEST_WAIT: Duration = Duration::from_secs(30);
/// How many of a process's last output lines are kept, to say why it died.
const KEPT_LINES: usize = 30;

/// Where a running process answers, and what proves a request is devpage's.
#[derive(Clone, Debug, PartialEq)]
pub struct Endpoint {
    /// The loopback port the process listens on.
    pub port: u16,
    /// The token devpage made for the process, sent as `X-Devpage-Token`.
    pub token: String,
}

/// The processes of one devpage. Cheap to clone: every clone is the same set.
#[derive(Clone)]
pub struct Processes(Arc<Shared>);

/// What every clone of a [`Processes`] shares.
struct Shared {
    /// devpage's own address, as a process is told it: `http://localhost:3000`.
    url: String,
    /// Who is at the browser: the local git name, sent as `X-Devpage-User`.
    user: String,
    /// Where devpage's own bun is once `devpage setup` installed it.
    bun: PathBuf,
    /// Whether a [`Live::key`] still names a bound extension.
    bound: fn(&str) -> bool,
    /// Each extension's process, by its [`Live::key`]. Held while one starts,
    /// so a second request waits for the first's process.
    slots: tokio::sync::Mutex<HashMap<String, Slot>>,
}

/// One extension's process, as last seen.
enum Slot {
    /// Started, and accepting connections when last asked.
    Running(Box<Running>),
    /// Dead: not started again before `until`, and `why` until then.
    Failed {
        until: Instant,
        failures: u32,
        why: String,
    },
}

/// A started process.
struct Running {
    /// The process; its standard input is held open until it is stopped.
    child: Child,
    /// Where it answers.
    endpoint: Endpoint,
    /// When it was started.
    started: Instant,
    /// How many times in a row it died before settling.
    failures: u32,
    /// Its last lines of output.
    output: Output,
}

/// A process's last lines of output, oldest first.
type Output = Arc<Mutex<VecDeque<String>>>;

impl Processes {
    /// The processes of the devpage at `url`, whose reader is `user`. A
    /// program named `bun` is devpage's own, at `bun`, when it is there.
    pub fn new(url: String, user: String, bun: PathBuf) -> Self {
        Self(Arc::new(Shared {
            url,
            user,
            bun,
            bound: extensions::bound,
            slots: Default::default(),
        }))
    }

    /// Who is at the browser.
    pub fn user(&self) -> &str {
        &self.0.user
    }

    /// Where the process of `live`'s extension answers, started first if it
    /// isn't running — or why it can't be had: it has none, it didn't start,
    /// or it keeps dying.
    pub async fn endpoint(&self, live: &Live) -> Result<Endpoint, String> {
        let Some(process) = &live.process else {
            return Err(format!("extension {} has no process", live.id));
        };
        let mut slots = self.0.slots.lock().await;
        self.forget_unbound(&mut slots).await;
        let failures = match slots.get_mut(&live.key) {
            Some(Slot::Running(running)) => match running.child.try_wait() {
                Ok(None) => return Ok(running.endpoint.clone()),
                _ if running.started.elapsed() >= SETTLED => 0,
                _ => running.failures,
            },
            Some(Slot::Failed { until, why, .. }) if Instant::now() < *until => {
                return Err(why.clone());
            }
            Some(Slot::Failed { failures, .. }) => *failures,
            None => 0,
        };
        if let Some(Slot::Running(running)) = slots.remove(&live.key) {
            stop(running.child).await;
        }
        match self.start(live, process, failures).await {
            Ok(running) => {
                let endpoint = running.endpoint.clone();
                slots.insert(live.key.clone(), Slot::Running(Box::new(running)));
                Ok(endpoint)
            }
            Err(why) => {
                let failures = failures + 1;
                let wait = Duration::from_secs(1 << failures.min(5)).min(LONGEST_WAIT);
                slots.insert(
                    live.key.clone(),
                    Slot::Failed {
                        until: Instant::now() + wait,
                        failures,
                        why: why.clone(),
                    },
                );
                Err(why)
            }
        }
    }

    /// Starts `process` for `live`'s extension and waits until it accepts
    /// connections; an error says why it didn't, with its last output.
    async fn start(
        &self,
        live: &Live,
        process: &extensions::Process,
        failures: u32,
    ) -> Result<Running, String> {
        let port = free_port().await.map_err(|error| error.to_string())?;
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let named = process.command.join(" ");
        // devpage's own bun, when `devpage setup` installed it, is the `bun`
        // a package names; the machine's is otherwise.
        let program = match process.command[0].as_str() {
            "bun" if self.0.bun.is_file() => self.0.bun.as_os_str(),
            program => program.as_ref(),
        };
        let mut command = Command::new(program);
        command
            .args(&process.command[1..])
            .current_dir(&process.folder)
            .env("DEVPAGE_PORT", port.to_string())
            .env("DEVPAGE_TOKEN", &token)
            .env("DEVPAGE_URL", &self.0.url)
            .env("DEVPAGE_CONFIG", &live.config)
            .env("DEVPAGE_ROOT", &live.folder)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        // In a group of its own, so stopping it stops what it started too.
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command
            .spawn()
            .map_err(|error| format!("extension {}: starting `{named}`: {error}", live.id))?;
        let output = Output::default();
        if let Some(out) = child.stdout.take() {
            tokio::spawn(follow(live.id.clone(), out, output.clone()));
        }
        if let Some(err) = child.stderr.take() {
            tokio::spawn(follow(live.id.clone(), err, output.clone()));
        }
        let started = Instant::now();
        let why = loop {
            if let Ok(Some(status)) = child.try_wait() {
                // Its output is read on other tasks; give them a moment.
                tokio::time::sleep(Duration::from_millis(50)).await;
                break format!("`{named}` ended ({status}) before listening on its port");
            }
            if TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
                return Ok(Running {
                    child,
                    endpoint: Endpoint { port, token },
                    started,
                    failures,
                    output,
                });
            }
            if started.elapsed() >= STARTING {
                break format!(
                    "`{named}` did not listen on DEVPAGE_PORT within {} seconds",
                    STARTING.as_secs()
                );
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        stop(child).await;
        let tail = tail(&output);
        Err(format!("extension {}: {why}\n{tail}", live.id))
    }

    /// Why the process of the extension `key` names died, if it has: its
    /// last output. Asked when a request to it failed.
    pub async fn died(&self, key: &str) -> Option<String> {
        let mut slots = self.0.slots.lock().await;
        match slots.get_mut(key) {
            Some(Slot::Running(running)) => match running.child.try_wait() {
                Ok(None) => None,
                _ => Some(tail(&running.output)),
            },
            _ => None,
        }
    }

    /// Stops every process whose extension is no longer bound.
    pub async fn forget(&self) {
        self.forget_unbound(&mut *self.0.slots.lock().await).await;
    }

    /// Stops, and drops from `slots`, every process whose extension is no
    /// longer bound.
    async fn forget_unbound(&self, slots: &mut HashMap<String, Slot>) {
        let gone: Vec<String> = slots
            .keys()
            .filter(|key| !(self.0.bound)(key))
            .cloned()
            .collect();
        for key in gone {
            if let Some(Slot::Running(running)) = slots.remove(&key) {
                stop(running.child).await;
            }
        }
    }

    /// Stops every process, as devpage exits.
    pub async fn stop(&self) {
        let mut slots = self.0.slots.lock().await;
        for (_, slot) in slots.drain() {
            if let Slot::Running(running) = slot {
                stop(running.child).await;
            }
        }
    }
}

/// A loopback port nothing listens on now.
async fn free_port() -> std::io::Result<u16> {
    Ok(TcpListener::bind(("127.0.0.1", 0))
        .await?
        .local_addr()?
        .port())
}

/// Copies each line `from` a process of the extension `id` to the terminal,
/// keeping the last in `output`.
async fn follow(id: String, from: impl AsyncRead + Unpin, output: Output) {
    let mut lines = BufReader::new(from).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        eprintln!("[{id}] {line}");
        let mut kept = output.lock().unwrap();
        if kept.len() == KEPT_LINES {
            kept.pop_front();
        }
        kept.push_back(line);
    }
}

/// The lines kept in `output`, one per line.
fn tail(output: &Output) -> String {
    let kept = output.lock().unwrap();
    kept.iter().cloned().collect::<Vec<_>>().join("\n")
}

/// Stops `child` and what it started: asked to end, then killed if it hasn't
/// within the Unix grace period.
async fn stop(mut child: Child) {
    #[cfg(unix)]
    if let Some(id) = child.id() {
        // The child leads its own process group, named by its id.
        unsafe { libc::kill(-(id as i32), libc::SIGTERM) };
        if tokio::time::timeout(STOPPING, child.wait()).await.is_ok() {
            return;
        }
        unsafe { libc::kill(-(id as i32), libc::SIGKILL) };
    }
    let _ = child.kill().await;
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::Path;

    /// The process the tests start: this test binary, running only
    /// [`a_process_serving`], which serves on `DEVPAGE_PORT` when it is set.
    pub(crate) fn serving(folder: &Path) -> extensions::Process {
        extensions::Process {
            command: vec![
                std::env::current_exe()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                "--exact".into(),
                "adapters::processes::tests::a_process_serving".into(),
                "--nocapture".into(),
            ],
            folder: folder.to_path_buf(),
        }
    }

    /// Processes that take every extension as bound, whatever the documents
    /// loaded say.
    pub(crate) fn keeping() -> Processes {
        Processes(Arc::new(Shared {
            url: "http://localhost:1".into(),
            user: "Ada".into(),
            bun: PathBuf::from("devpage-no-bun"),
            bound: |_| true,
            slots: Default::default(),
        }))
    }

    /// An address under the `live/` of an extension bound nowhere, answered by
    /// `process`.
    pub(crate) fn live(key: &str, process: Option<extensions::Process>) -> Live {
        Live {
            key: key.into(),
            id: "toy".into(),
            process,
            config: "{\"greeting\":\"hi\"}".into(),
            folder: std::env::temp_dir(),
            rest: String::new(),
        }
    }

    /// Not a test of its own: the process the others start. It answers every
    /// request with what it was started with and what the request carried,
    /// and echoes whatever an upgraded connection sends; a request under
    /// `/pages/` it answers with a page in Markdown — but `gone.md` it has
    /// not, and `broken.md` it fails at. With `die` in its configuration it
    /// ends at once instead.
    #[test]
    fn a_process_serving() {
        let Ok(port) = std::env::var("DEVPAGE_PORT") else {
            return;
        };
        if std::env::var("DEVPAGE_CONFIG").is_ok_and(|config| config.contains("die")) {
            eprintln!("no reason to live");
            std::process::exit(3);
        }
        use axum::extract::Request;
        use axum::http::{StatusCode, header};
        use axum::response::{IntoResponse, Response};
        async fn answer(mut request: Request) -> Response {
            let headers = request.headers().clone();
            let said = move |name: &str| {
                headers
                    .get(name)
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("-")
                    .to_string()
            };
            if request.headers().contains_key(header::UPGRADE) {
                let upgrade = hyper::upgrade::on(&mut request);
                tokio::spawn(async move {
                    let Ok(upgraded) = upgrade.await else {
                        return;
                    };
                    let (mut read, mut write) =
                        tokio::io::split(hyper_util::rt::TokioIo::new(upgraded));
                    let _ = tokio::io::copy(&mut read, &mut write).await;
                });
                return (
                    StatusCode::SWITCHING_PROTOCOLS,
                    [(header::CONNECTION, "upgrade"), (header::UPGRADE, "echo")],
                )
                    .into_response();
            }
            let path = request.uri().path().to_string();
            if path.starts_with("/pages/") {
                return match path.as_str() {
                    "/pages/gone.md" => (StatusCode::NOT_FOUND, "no such page").into_response(),
                    "/pages/broken.md" => StatusCode::IM_A_TEAPOT.into_response(),
                    _ => format!(
                        "# Written\n\nAsked for `{path}` by {} with the token: {}.\n",
                        said("x-devpage-user"),
                        said("x-devpage-token") == std::env::var("DEVPAGE_TOKEN").unwrap(),
                    )
                    .into_response(),
                };
            }
            let line = format!(
                "{} {}?{} token={} user={} config={} url={} root={}",
                request.method(),
                request.uri().path(),
                request.uri().query().unwrap_or(""),
                said("x-devpage-token") == std::env::var("DEVPAGE_TOKEN").unwrap(),
                said("x-devpage-user"),
                std::env::var("DEVPAGE_CONFIG").unwrap(),
                std::env::var("DEVPAGE_URL").unwrap(),
                std::env::var("DEVPAGE_ROOT").is_ok(),
            );
            let body = axum::body::to_bytes(request.into_body(), 1 << 20)
                .await
                .unwrap();
            format!("{line} body={}", String::from_utf8_lossy(&body)).into_response()
        }
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let listener = TcpListener::bind(format!("127.0.0.1:{port}"))
                .await
                .unwrap();
            axum::serve(listener, axum::Router::new().fallback(answer))
                .await
                .unwrap();
        });
    }

    #[tokio::test]
    async fn a_process_starts_once_is_restarted_when_it_died_and_stops() {
        let processes = Processes::new(
            "http://localhost:1".into(),
            "Ada".into(),
            PathBuf::from("devpage-no-bun"),
        );
        let live = live("0000000000000000/toy", Some(serving(&std::env::temp_dir())));
        let first = processes.start(&live, live.process.as_ref().unwrap(), 0);
        let mut running = first.await.unwrap();
        assert!(
            TcpStream::connect(("127.0.0.1", running.endpoint.port))
                .await
                .is_ok()
        );
        assert_eq!(running.endpoint.token.len(), 64);
        let port = running.endpoint.port;
        // Killed behind devpage's back, it is seen dead.
        running.child.kill().await.unwrap();
        assert!(running.child.try_wait().unwrap().is_some());
        stop(running.child).await;
        assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());
        // An extension bound nowhere keeps no process: asking stops it again.
        let endpoint = processes.endpoint(&live).await.unwrap();
        processes.forget().await;
        assert!(
            TcpStream::connect(("127.0.0.1", endpoint.port))
                .await
                .is_err()
        );
        assert!(processes.0.slots.lock().await.is_empty());
    }

    #[tokio::test]
    async fn a_program_named_bun_is_devpages_own_bun_when_it_is_installed() {
        // devpage's bun is this test binary here, so the process serves.
        let processes = Processes::new(
            "http://localhost:1".into(),
            "Ada".into(),
            std::env::current_exe().unwrap(),
        );
        let mut process = serving(&std::env::temp_dir());
        process.command[0] = "bun".into();
        let live = live("4444444444444444/toy", Some(process));
        let running = processes.start(&live, live.process.as_ref().unwrap(), 0);
        let running = running.await.unwrap();
        assert!(
            TcpStream::connect(("127.0.0.1", running.endpoint.port))
                .await
                .is_ok()
        );
        stop(running.child).await;
    }

    #[tokio::test]
    async fn a_process_that_dies_says_why_and_is_not_started_again_at_once() {
        let processes = keeping();
        let mut live = live("1111111111111111/toy", Some(serving(&std::env::temp_dir())));
        live.config = "{\"die\":true}".into();
        let why = processes.endpoint(&live).await.unwrap_err();
        assert!(why.contains("before listening on its port"), "{why}");
        assert!(why.contains("no reason to live"), "{why}");
        // Able to live now, it is still not started before its wait is over.
        live.config = "{}".into();
        assert_eq!(processes.endpoint(&live).await.unwrap_err(), why);
        let none = super::tests::live("2222222222222222/toy", None);
        let why = processes.endpoint(&none).await.unwrap_err();
        assert_eq!(why, "extension toy has no process");
        let missing = extensions::Process {
            command: vec!["devpage-no-such-program".into()],
            folder: std::env::temp_dir(),
        };
        let why = processes
            .endpoint(&super::tests::live("3333333333333333/toy", Some(missing)))
            .await
            .unwrap_err();
        assert!(why.contains("starting `devpage-no-such-program`"), "{why}");
    }
}

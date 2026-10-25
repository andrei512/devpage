//! Designs in the server: resolving `.design` files from the served folder
//! and painting them as PNGs with the design engine.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use devpage_design::{Files, Renderer, Scene};

/// Reads design files from the served folder, never outside it.
pub struct Served(pub PathBuf);

impl Files for Served {
    fn read(&self, path: &str) -> Option<String> {
        let file = self
            .0
            .join(path.trim_start_matches('/'))
            .canonicalize()
            .ok()?;
        if !file.starts_with(&self.0) {
            return None;
        }
        std::fs::read_to_string(file).ok()
    }
}

/// The engine's renderer, shared by every request: its fonts load once.
#[derive(Clone)]
pub struct Designs {
    served: Arc<Served>,
    renderer: Arc<Mutex<Renderer>>,
}

impl Designs {
    pub fn new(served: PathBuf) -> Designs {
        Designs {
            served: Arc::new(Served(served)),
            renderer: Arc::new(Mutex::new(Renderer::new())),
        }
    }

    /// The design at `path` resolved, or its problems one per line.
    pub fn resolve(&self, path: &str) -> Result<Scene, String> {
        devpage_design::resolve(&*self.served, path.trim_start_matches('/')).map_err(|problems| {
            problems
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        })
    }

    /// The design at `path` painted as a PNG.
    pub async fn png(&self, path: String) -> Result<Vec<u8>, String> {
        let designs = self.clone();
        tokio::task::spawn_blocking(move || {
            let scene = designs.resolve(&path)?;
            let mut renderer = designs.renderer.lock().expect("renderer");
            Ok(devpage_design::png(&mut renderer, &scene))
        })
        .await
        .expect("painting")
    }
}

pub mod service {
    tonic::include_proto!("devpage.design.v1");
}

use std::pin::Pin;

use devpage_design::proto::{Patch, Problems, Update, WatchRequest, update::Body};
use devpage_design::wire;
use tokio::sync::broadcast;
use tokio_stream::Stream;
use tonic::{Request, Response, Status};

/// The watch stream: the scene, then a patch after each change that altered it.
pub struct Watch {
    pub designs: Designs,
    pub changes: broadcast::Sender<String>,
}

#[tonic::async_trait]
impl service::designs_server::Designs for Watch {
    type WatchStream = Pin<Box<dyn Stream<Item = Result<Update, Status>> + Send>>;

    async fn watch(
        &self,
        request: Request<WatchRequest>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        let path = request.into_inner().path;
        let designs = self.designs.clone();
        let mut changes = self.changes.subscribe();
        let stream = async_stream(move |tx| async move {
            let mut last: Option<devpage_design::resolve::Node> = None;
            loop {
                let update = match designs.resolve(&path) {
                    Ok(scene) => match &last {
                        None => Some(Body::Scene(wire::encode(&scene.root))),
                        Some(old) => {
                            let replace = wire::diff(old, &scene.root);
                            (!replace.is_empty()).then_some(Body::Patch(Patch { replace }))
                        }
                    }
                    .inspect(|_| last = Some(scene.root)),
                    Err(problems) => Some(Body::Problems(Problems {
                        problems: problems.lines().map(String::from).collect(),
                    })),
                };
                if let Some(body) = update
                    && tx.send(Ok(Update { body: Some(body) })).await.is_err()
                {
                    return;
                }
                // Any change, or falling behind, means: look again.
                if let Err(broadcast::error::RecvError::Closed) = changes.recv().await {
                    return;
                }
            }
        });
        Ok(Response::new(Box::pin(stream)))
    }
}

/// A stream fed by a task through a channel.
fn async_stream<F, Fut>(f: F) -> impl Stream<Item = Result<Update, Status>>
where
    F: FnOnce(tokio::sync::mpsc::Sender<Result<Update, Status>>) -> Fut,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    tokio::spawn(f(tx));
    tokio_stream::wrappers::ReceiverStream::new(rx)
}

/// The canvas page for the design at `path`.
pub fn page(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><title>{name}</title>
<meta name="viewport" content="width=device-width, initial-scale=1">
<link rel="stylesheet" href="/__devpage/style.css">
<style>body{{margin:0;min-height:100vh;display:grid;place-items:center;background:var(--bg, #e2e8f0)}}canvas{{box-shadow:0 8px 32px #0f172a26}}</style>
</head><body>
<canvas data-design="/{path}"></canvas>
<script type="module" src="/__devpage/design.js"></script>
</body></html>"#,
        path = path.trim_start_matches('/'),
    )
}

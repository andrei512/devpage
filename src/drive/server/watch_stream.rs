//! The watch stream: the gRPC-Web `Watch` call that carries each design a
//! page shows to the browser — its scene, then what changed, or its
//! problems — as the design store announces changes.

use std::pin::Pin;

use devpage_design::Problem;
use devpage_design::proto::{Problems, Update, WatchRequest, update::Body};
use tokio::sync::broadcast;
use tokio_stream::Stream;
use tonic::{Request, Response, Status};

use crate::designs::{self, Designs, Next};

/// The watch stream: one per page, over every design the page shows. For
/// each, the scene, then a patch after each change that altered it, or its
/// problems. A deleted design stays watched: its `no such file` is a problem
/// like any other, and writing it again brings it back.
pub struct Watch {
    designs: Designs,
    /// The address of each design whose scene or problems changed, as the
    /// store tells it.
    updates: broadcast::Sender<String>,
}

impl Watch {
    /// The watch stream over `designs`, hearing what [`hear`] passes on.
    pub fn new(designs: Designs, updates: broadcast::Sender<String>) -> Self {
        Watch { designs, updates }
    }
}

/// Where the watch streams hear each change `designs` tells: one channel
/// per server, so the store gets one listener however often the routes are
/// built.
pub fn hear(designs: &Designs) -> broadcast::Sender<String> {
    let updates = broadcast::channel(64).0;
    let told = updates.clone();
    designs.on_change(move |path| {
        let _ = told.send(path.to_string());
    });
    updates
}

/// What one design's watcher last sent: the version of the scene the
/// browser holds, and the problems it shows.
#[derive(Default)]
struct Sent {
    version: Option<u64>,
    problems: Option<Vec<Problem>>,
}

impl Sent {
    /// The update `next` calls for after what was sent, if any.
    fn next(&mut self, next: Next) -> Option<Body> {
        match next {
            Next::Problems(problems) if self.problems.as_ref() != Some(&problems) => {
                self.problems = Some(problems.clone());
                Some(Body::Problems(Problems {
                    problems: problems.iter().map(Problem::to_string).collect(),
                }))
            }
            Next::Problems(_) => None,
            Next::Scene(version, scene) => {
                self.version = Some(version);
                self.problems = None;
                Some(Body::Scene(*scene))
            }
            Next::Patch(version, patch) => {
                self.version = Some(version);
                self.problems = None;
                Some(Body::Patch(patch))
            }
            // Problems on show: even an empty patch says the design is valid
            // again.
            Next::Current(themed) => self.problems.take().map(|_| {
                Body::Patch(devpage_design::proto::Patch {
                    themed,
                    ..Default::default()
                })
            }),
        }
    }
}

#[tonic::async_trait]
impl devpage_design::proto::designs_server::Designs for Watch {
    type WatchStream = Pin<Box<dyn Stream<Item = Result<Update, Status>> + Send>>;

    async fn watch(
        &self,
        request: Request<WatchRequest>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        // Each design once, keyed by its address, answered with the name the
        // request gave it.
        let mut watched: Vec<(String, String, Sent)> = Vec::new();
        for named in request.into_inner().paths {
            let path = designs::address(&named).to_string();
            if !watched.iter().any(|(p, _, _)| *p == path) {
                watched.push((path, named, Sent::default()));
            }
        }
        let designs = self.designs.clone();
        let mut updates = self.updates.subscribe();
        let (tx, rx) = tokio::sync::mpsc::channel(16);
        tokio::spawn(async move {
            let mut due: Vec<String> = watched.iter().map(|(p, _, _)| p.clone()).collect();
            loop {
                for path in due.drain(..) {
                    let Some(index) = watched.iter().position(|(p, _, _)| *p == path) else {
                        continue;
                    };
                    let (_, named, sent) = &mut watched[index];
                    let next = designs.next(&path, sent.version);
                    if let Some(body) = sent.next(next) {
                        let update = Update {
                            body: Some(body),
                            path: named.clone(),
                        };
                        if tx.send(Ok(update)).await.is_err() {
                            return;
                        }
                    }
                }
                if watched.is_empty() {
                    return;
                }
                // Wait for a watched design to change; falling behind means
                // look at every one again.
                match updates.recv().await {
                    Ok(changed) => due.push(changed),
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        due = watched.iter().map(|(p, _, _)| p.clone()).collect();
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        });
        Ok(Response::new(Box::pin(
            tokio_stream::wrappers::ReceiverStream::new(rx),
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use prost::Message;

    use super::*;
    use crate::server::design_tests::start;
    use devpage_core::designs::testing::{BUTTON, OTHER, SCREEN, THEMES, save, served};

    /// A change that only makes a scene themed, or no longer themed, is
    /// still sent: it puts the switch on an embed, or takes it off.
    #[test]
    fn a_scene_that_becomes_themed_is_sent() {
        let dir = served("becomes-themed");
        std::fs::write(
            dir.join("app/themes.yml"),
            THEMES.replace("#000000", "#ffffff"),
        )
        .unwrap();
        let file = dir.join("app/a.design");
        std::fs::write(&file, "<Frame fill=\"#ffffff\" />").unwrap();
        let designs = Designs::new(dir.clone());
        let mut sent = Sent::default();
        let next = |sent: &mut Sent| sent.next(designs.next("app/a.design", sent.version));
        assert!(matches!(next(&mut sent), Some(Body::Scene(_))));
        save(&designs, &file, "<Frame fill={theme.bg} />");
        let Some(Body::Patch(patch)) = next(&mut sent) else {
            panic!("a patch")
        };
        assert!(patch.themed);
        assert!(next(&mut sent).is_none());
    }

    /// Every stream of a design sends the patch the store made once; a
    /// stream that joins later is sent the scene whole.
    #[test]
    fn every_stream_sends_the_one_patch_and_a_late_one_the_scene() {
        let dir = served("one-patch");
        let designs = Designs::new(dir.clone());
        let (mut a, mut b) = (Sent::default(), Sent::default());
        let path = "app/screen.design";
        assert!(matches!(
            a.next(designs.next(path, a.version)),
            Some(Body::Scene(_))
        ));
        assert!(matches!(
            b.next(designs.next(path, b.version)),
            Some(Body::Scene(_))
        ));
        save(
            &designs,
            &dir.join("app/screen.design"),
            &SCREEN.replace("Cart", "Basket"),
        );
        let Some(Body::Patch(pa)) = a.next(designs.next(path, a.version)) else {
            panic!()
        };
        let Some(Body::Patch(pb)) = b.next(designs.next(path, b.version)) else {
            panic!()
        };
        assert_eq!(pa, pb);
        assert_eq!(pa.ops.len(), 1);
        let mut late = Sent::default();
        assert!(matches!(
            late.next(designs.next(path, late.version)),
            Some(Body::Scene(_))
        ));
    }

    /// The id of the `i`th child of a scene's root, as the wire carries it.
    fn scene_root_child(
        root: &devpage_design::proto::Node,
        i: usize,
    ) -> Option<devpage_design::proto::NodeId> {
        root.children[i].id.clone()
    }

    #[tokio::test]
    async fn the_design_service_answers_only_watch() {
        let dir = served("only-watch");
        let (address, _) = start(&dir, "app/screen.design").await;
        let client = reqwest::Client::new();
        let other = client
            .post(format!("{address}/devpage.design.v1.Designs/Other"))
            .header("content-type", "application/grpc-web+proto")
            .body(vec![0u8, 0, 0, 0, 0])
            .send()
            .await
            .unwrap();
        assert!(other.status().is_client_error(), "{}", other.status());
    }

    /// A gRPC-Web `Watch` call, read frame by frame.
    struct Stream {
        response: reqwest::Response,
        buffer: Vec<u8>,
    }

    impl Stream {
        async fn open(address: &str, paths: &[&str]) -> Stream {
            let request = WatchRequest {
                paths: paths.iter().map(|p| p.to_string()).collect(),
            }
            .encode_to_vec();
            let mut body = vec![0];
            body.extend((request.len() as u32).to_be_bytes());
            body.extend(request);
            let response = reqwest::Client::new()
                .post(format!("{address}/devpage.design.v1.Designs/Watch"))
                .header("content-type", "application/grpc-web+proto")
                .header("x-grpc-web", "1")
                .body(body)
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            Stream {
                response,
                buffer: Vec::new(),
            }
        }

        /// The next frame's body: `Ok(body)`, `Err(trailers)` at the end, or
        /// `None` when nothing arrives within a second.
        async fn next(&mut self) -> Option<Result<Body, String>> {
            self.update()
                .await
                .map(|update| update.map(|update| update.body.unwrap()))
        }

        /// The next frame, whole.
        async fn update(&mut self) -> Option<Result<Update, String>> {
            loop {
                if self.buffer.len() >= 5 {
                    let length = u32::from_be_bytes(self.buffer[1..5].try_into().unwrap()) as usize;
                    if self.buffer.len() >= 5 + length {
                        let flag = self.buffer[0];
                        let payload: Vec<u8> = self.buffer.drain(..5 + length).skip(5).collect();
                        return Some(if flag & 0x80 != 0 {
                            Err(String::from_utf8_lossy(&payload).into_owned())
                        } else {
                            Ok(Update::decode(&payload[..]).unwrap())
                        });
                    }
                }
                let chunk =
                    tokio::time::timeout(Duration::from_secs(1), self.response.chunk()).await;
                match chunk {
                    Ok(Ok(Some(bytes))) => self.buffer.extend_from_slice(&bytes),
                    Ok(_) | Err(_) => return None,
                }
            }
        }
    }

    #[tokio::test]
    async fn the_watch_stream_sends_the_scene_then_only_what_changed() {
        let dir = served("watch");
        let (address, designs) = start(&dir, "app/screen.design").await;
        let mut stream = Stream::open(&address, &["/app/screen.design"]).await;

        let Some(Ok(Body::Scene(scene))) = stream.next().await else {
            panic!("the first update is the scene")
        };
        let scene_ids = scene.root.unwrap();
        assert_eq!(scene_ids.children.len(), 2);

        // A save of the imported button: a patch of the button's instance only.
        save(
            &designs,
            &dir.join("app/button.design"),
            &BUTTON.replace("#2563eb", "#16a34a"),
        );
        let Some(Ok(Body::Patch(patch))) = stream.next().await else {
            panic!("a patch")
        };
        // One change, to the instance's own attributes, by its id.
        assert_eq!(patch.ops.len(), 1, "{patch:?}");
        let Some(devpage_design::proto::op::Op::Change(button)) = &patch.ops[0].op else {
            panic!("{patch:?}")
        };
        assert_eq!(button.id, scene_root_child(&scene_ids, 1));

        // A save that changes nothing in the scene sends nothing.
        save(
            &designs,
            &dir.join("app/button.design"),
            &format!("// a comment\n{}", BUTTON.replace("#2563eb", "#16a34a")),
        );
        assert!(stream.next().await.is_none());

        // Problems, as their own update.
        save(
            &designs,
            &dir.join("app/screen.design"),
            &SCREEN.replace("<Button label", "<Button lable"),
        );
        let Some(Ok(Body::Problems(problems))) = stream.next().await else {
            panic!("problems")
        };
        assert!(problems.problems[0].contains("lable"), "{problems:?}");

        // The typo undone: the scene is the one last sent, and an empty patch
        // says the design is valid again.
        save(&designs, &dir.join("app/screen.design"), SCREEN);
        let Some(Ok(Body::Patch(patch))) = stream.next().await else {
            panic!("an empty patch after the undo")
        };
        assert!(patch.ops.is_empty(), "{patch:?}");

        // Problems again, then a fix that changes the scene: a patch against
        // the last scene sent.
        save(
            &designs,
            &dir.join("app/screen.design"),
            &SCREEN.replace("<Button label", "<Button lable"),
        );
        assert!(matches!(stream.next().await, Some(Ok(Body::Problems(_)))));
        save(
            &designs,
            &dir.join("app/screen.design"),
            &SCREEN.replace("Cart", "Basket"),
        );
        let Some(Ok(Body::Patch(patch))) = stream.next().await else {
            panic!("a patch after the fix")
        };
        let Some(devpage_design::proto::op::Op::Change(heading)) = &patch.ops[0].op else {
            panic!("{patch:?}")
        };
        assert_eq!(heading.id, scene_root_child(&scene_ids, 0));

        // Deleted: the problem, and the stream stays open for it…
        std::fs::remove_file(dir.join("app/screen.design")).unwrap();
        designs.changed(&dir.join("app/screen.design"));
        let Some(Ok(Body::Problems(problems))) = stream.next().await else {
            panic!("the deleted design's problem")
        };
        assert!(
            problems.problems[0].contains("no such file"),
            "{problems:?}"
        );
        // …so writing it again brings it back.
        save(&designs, &dir.join("app/screen.design"), SCREEN);
        assert!(
            matches!(stream.next().await, Some(Ok(Body::Patch(_)))),
            "the design back"
        );
    }

    #[tokio::test]
    async fn one_stream_watches_every_design_a_page_shows() {
        let dir = served("many");
        std::fs::write(
            dir.join("app/other.design"),
            SCREEN.replace("Cart", "Other"),
        )
        .unwrap();
        let (address, designs) = start(&dir, "app/screen.design").await;
        let mut stream = Stream::open(
            &address,
            &[
                "/app/screen.design",
                "app/other.design",
                "app/screen.design",
            ],
        )
        .await;

        // Each design's scene, named as the request named it, once.
        let mut first = Vec::new();
        for _ in 0..2 {
            let Some(Ok(update)) = stream.update().await else {
                panic!("a scene for each design")
            };
            assert!(matches!(update.body, Some(Body::Scene(_))), "{update:?}");
            first.push(update.path);
        }
        assert_eq!(first, ["/app/screen.design", "app/other.design"]);
        assert!(stream.update().await.is_none(), "no third scene");

        // An edit to one: a patch naming it.
        save(
            &designs,
            &dir.join("app/other.design"),
            &SCREEN.replace("Cart", "Elsewhere"),
        );
        let Some(Ok(update)) = stream.update().await else {
            panic!("a patch")
        };
        assert_eq!(update.path, "app/other.design");
        assert!(matches!(update.body, Some(Body::Patch(_))));

        // An edit to the button both use: a patch for each.
        save(
            &designs,
            &dir.join("app/button.design"),
            &BUTTON.replace("#2563eb", "#16a34a"),
        );
        let mut patched = Vec::new();
        for _ in 0..2 {
            let Some(Ok(update)) = stream.update().await else {
                panic!("a patch for each design")
            };
            assert!(matches!(update.body, Some(Body::Patch(_))));
            patched.push(update.path);
        }
        patched.sort();
        assert_eq!(patched, ["/app/screen.design", "app/other.design"]);

        // One deleted: it says so, and the stream goes on for the other.
        std::fs::remove_file(dir.join("app/other.design")).unwrap();
        designs.changed(&dir.join("app/other.design"));
        let Some(Ok(update)) = stream.update().await else {
            panic!("the deleted design's problem")
        };
        assert_eq!(update.path, "app/other.design");
        let Some(Body::Problems(problems)) = update.body else {
            panic!("its problem")
        };
        assert!(
            problems.problems[0].contains("no such file"),
            "{problems:?}"
        );
        save(
            &designs,
            &dir.join("app/screen.design"),
            &SCREEN.replace("Cart", "Basket"),
        );
        let Some(Ok(update)) = stream.update().await else {
            panic!("the stream goes on for the design left")
        };
        assert_eq!(update.path, "/app/screen.design");

        // Written again — a branch switch, an agent rewriting it — it comes
        // back on the same stream: a patch against the last scene sent.
        save(&designs, &dir.join("app/other.design"), OTHER);
        let Some(Ok(update)) = stream.update().await else {
            panic!("the re-created design")
        };
        assert_eq!(update.path, "app/other.design");
        assert!(matches!(update.body, Some(Body::Patch(_))), "{update:?}");
    }
}

//! The watch stream: the gRPC-Web `Watch` call that carries each design a
//! page shows to the browser — its scene, then what changed, or its
//! problems — as the design store announces changes.

use std::pin::Pin;

use devpage_design::proto::{Problems, Update, WatchRequest, update::Body};
use devpage_design::{Problem, Scene, wire};
use tokio::sync::broadcast;
use tokio_stream::Stream;
use tonic::{Request, Response, Status};

use crate::designs::{self, Designs};

/// The watch stream: one per page, over every design the page shows. For
/// each, the scene, then a patch after each change that altered it, or its
/// problems. A deleted design stays watched: its `no such file` is a problem
/// like any other, and writing it again brings it back.
pub struct Watch {
    pub designs: Designs,
}

/// A scene encoded for the stream: whole, the first time, then as the
/// subtrees that changed since.
enum Encoded {
    Whole(Box<devpage_design::proto::Scene>),
    Changed(devpage_design::proto::Patch),
}

/// What one design's watcher last sent.
#[derive(Default)]
struct Sent {
    /// The scene the browser holds — and so the image files it has.
    scene: Option<Scene>,
    problems: Option<Vec<Problem>>,
}

impl Sent {
    /// The update `resolved` calls for after what was sent, if any. `path`
    /// names the design should encoding it ever fail.
    fn next(&mut self, path: &str, resolved: Result<Scene, Vec<Problem>>) -> Option<Body> {
        let result = resolved.and_then(|scene| {
            // Encoding is a call into the engine too, so it is guarded.
            let old = self.scene.as_ref();
            let body = designs::guarded(
                path,
                || {
                    Ok(match old {
                        None => Encoded::Whole(Box::new(wire::scene(&scene))),
                        Some(old) => Encoded::Changed(wire::patch(old, &scene)),
                    })
                },
                |problem| vec![problem],
            )?;
            Ok((scene, body))
        });
        match result {
            Ok((scene, body)) => {
                // Problems on show: even an empty patch says the design is
                // valid again.
                let fixed = self.problems.take().is_some();
                let themed = self.scene.as_ref().map(|s| s.themed);
                let body = match body {
                    Encoded::Whole(whole) => Some(Body::Scene(*whole)),
                    Encoded::Changed(patch) => {
                        (fixed || !patch.replace.is_empty() || themed != Some(patch.themed))
                            .then_some(Body::Patch(patch))
                    }
                };
                self.scene = Some(scene);
                body
            }
            Err(problems) if self.problems.as_ref() != Some(&problems) => {
                self.problems = Some(problems.clone());
                Some(Body::Problems(Problems {
                    problems: problems.iter().map(Problem::to_string).collect(),
                }))
            }
            Err(_) => None,
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
        let mut updates = designs.changes();
        let (tx, rx) = tokio::sync::mpsc::channel(16);
        tokio::spawn(async move {
            let mut due: Vec<String> = watched.iter().map(|(p, _, _)| p.clone()).collect();
            loop {
                for path in due.drain(..) {
                    let Some(index) = watched.iter().position(|(p, _, _)| *p == path) else {
                        continue;
                    };
                    let resolved = designs.resolve(&path);
                    let (_, named, sent) = &mut watched[index];
                    if let Some(body) = sent.next(&path, resolved) {
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
    use std::collections::HashMap;
    use std::time::Duration;

    use prost::Message;

    use super::*;
    use crate::designs::tests::{BUTTON, OTHER, SCREEN, THEMES, save, served, start};

    /// A change that only makes a scene themed, or no longer themed, is
    /// still sent: it puts the switch on an embed, or takes it off.
    #[test]
    fn a_scene_that_becomes_themed_is_sent() {
        let files: HashMap<String, String> = [
            (
                "themes.yml".to_string(),
                THEMES.replace("#000000", "#ffffff"),
            ),
            (
                "a.design".to_string(),
                "<Frame fill=\"#ffffff\" />".to_string(),
            ),
            (
                "b.design".to_string(),
                "<Frame fill={theme.bg} />".to_string(),
            ),
        ]
        .into();
        let resolved = |path: &str| {
            devpage_design::resolve(&files, path).map(|mut scene| {
                scene.files.clear();
                scene
            })
        };
        let mut sent = Sent::default();
        assert!(matches!(
            sent.next("a", resolved("a.design")),
            Some(Body::Scene(_))
        ));
        let Some(Body::Patch(patch)) = sent.next("a", resolved("b.design")) else {
            panic!("a patch")
        };
        assert!(patch.replace.is_empty() && patch.themed);
        assert!(sent.next("a", resolved("b.design")).is_none());
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
        assert_eq!(scene.root.unwrap().children.len(), 2);

        // A save of the imported button: a patch of the button's instance only.
        save(
            &designs,
            &dir.join("app/button.design"),
            &BUTTON.replace("#2563eb", "#16a34a"),
        );
        let Some(Ok(Body::Patch(patch))) = stream.next().await else {
            panic!("a patch")
        };
        assert_eq!(
            patch
                .replace
                .iter()
                .map(|r| r.id.as_str())
                .collect::<Vec<_>>(),
            vec!["0.1"]
        );

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
        assert!(patch.replace.is_empty(), "{patch:?}");

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
        assert_eq!(patch.replace[0].id, "0.0");

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

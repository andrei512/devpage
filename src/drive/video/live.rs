//! The narrator: a GPT-Live session, on OpenAI or on Azure, chosen by
//! `DEVPAGE_` environment variables, that says each slide's notes word for
//! word. Its voice streams into [`Heard`] as it arrives.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use async_tungstenite::tungstenite::Message;
use async_tungstenite::tungstenite::client::IntoClientRequest;
use async_tungstenite::tungstenite::http::{HeaderValue, Request};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use futures::StreamExt;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use super::Heard;
use super::silence::RATE;

use crate::document::Narration;

pub const OPENAI_API_KEY: &str = "DEVPAGE_OPENAI_API_KEY";
pub const AZURE_ENDPOINT: &str = "DEVPAGE_AZURE_OPENAI_ENDPOINT";
pub const AZURE_API_KEY: &str = "DEVPAGE_AZURE_OPENAI_API_KEY";
pub const MODEL: &str = "DEVPAGE_VIDEO_MODEL";
/// The model when `DEVPAGE_VIDEO_MODEL` names none.
pub const DEFAULT_MODEL: &str = "gpt-live-1";
/// The narrator's voice.
const VOICE: &str = "marin";

/// Where GPT-Live runs, and the key it takes.
#[derive(PartialEq)]
pub enum Provider {
    OpenAi {
        key: String,
    },
    /// An Azure Foundry resource, by its host name.
    Azure {
        host: String,
        key: String,
    },
}

/// The provider and the model a run narrates with. It holds a key, so it is
/// never printed.
pub struct Config {
    pub provider: Provider,
    /// On Azure, the deployment's name.
    pub model: String,
}

impl Config {
    /// Reads the provider from the environment: the Azure endpoint being set
    /// selects Azure; otherwise the OpenAI key selects OpenAI; neither is an
    /// error naming the variables. `OPENAI_API_KEY` is never read.
    pub fn from_env() -> Result<Self> {
        Self::from_vars(|name| std::env::var(name).ok())
    }

    fn from_vars(var: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let var = |name: &str| var(name).filter(|value| !value.trim().is_empty());
        let model = var(MODEL).unwrap_or_else(|| DEFAULT_MODEL.to_string());
        let provider = if let Some(endpoint) = var(AZURE_ENDPOINT) {
            let Some(key) = var(AZURE_API_KEY) else {
                bail!("{AZURE_ENDPOINT} is set but {AZURE_API_KEY} is not");
            };
            Provider::Azure {
                host: azure_host(&endpoint)?,
                key,
            }
        } else if let Some(key) = var(OPENAI_API_KEY) {
            Provider::OpenAi { key }
        } else {
            bail!(
                "video needs GPT-Live: set {OPENAI_API_KEY}, or {AZURE_ENDPOINT} and {AZURE_API_KEY}"
            );
        };
        Ok(Self { provider, model })
    }

    /// The WebSocket address of a live session.
    pub fn url(&self) -> String {
        match &self.provider {
            Provider::OpenAi { .. } => "wss://api.openai.com/v1/live/sessions".to_string(),
            Provider::Azure { host, .. } => format!("wss://{host}/openai/v1/live/sessions"),
        }
    }

    /// The request that opens a live session, its key in the provider's
    /// header.
    pub fn request(&self) -> Result<Request<()>> {
        let mut request = self.url().into_client_request()?;
        let (name, value) = match &self.provider {
            Provider::OpenAi { key } => ("authorization", format!("Bearer {key}")),
            Provider::Azure { key, .. } => ("api-key", key.clone()),
        };
        let mut value = HeaderValue::from_str(&value).context("the API key is not a header")?;
        value.set_sensitive(true);
        request.headers_mut().insert(name, value);
        Ok(request)
    }
}

/// The host of an Azure endpoint, given as an address
/// (`https://<resource>.openai.azure.com/`, any path) or a bare host name.
fn azure_host(endpoint: &str) -> Result<String> {
    let endpoint = endpoint.trim();
    let address = if endpoint.contains("://") {
        endpoint.to_string()
    } else {
        format!("https://{endpoint}")
    };
    let url = reqwest::Url::parse(&address)
        .ok()
        .filter(|url| url.host_str().is_some_and(|host| !host.is_empty()))
        .with_context(|| format!("{AZURE_ENDPOINT} is not an address: {endpoint}"))?;
    let host = url.host_str().unwrap_or_default();
    Ok(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    })
}

/// The tone a deck without `tone` is read in.
pub const DEFAULT_TONE: &str = "Natural and upbeat, like a speaker who enjoys this talk. Engaged and warm, never flat and never salesy.";

/// What the narrator is told once, before any slide, following OpenAI's
/// GPT-Live prompt template: who it is, the talk (audience, goal, tone), how
/// to speak, its task, then the backchannel, interruption and delegation
/// policies. Everything but the deck's fields is fixed.
pub fn instructions(title: &str, count: usize, narration: &Narration) -> String {
    let field = |value: &Option<String>| value.as_deref().map(str::trim).unwrap_or("").to_string();
    let tone = narration
        .tone
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or(DEFAULT_TONE);
    let pronounce = narration
        .pronounce
        .iter()
        .map(|line| format!("\n{}", line.trim()))
        .collect::<String>();
    format!(
        "You are the narrator of \"{title}\", a recorded talk of {count} slides.

Audience: {audience}
Goal: {goal}
Tone: {tone}

Read like a speaker who enjoys giving this talk, in the tone above, for this audience. Speak clearly at an unhurried pace. Lean into the words that carry each slide's point.{pronounce}

Each slide arrives as a passage with its delivery. Read the passage word for word once, with natural phrasing and complete word endings, in that delivery. Say only the passage, without an introduction, closing remark, transition, comment, or delegation. Then stay silent until the next passage arrives.

Backchannel policy: Never backchannel.
Interruption policy: There is no live listener. Input audio is only silence or noise. Do not treat a cough, music, or any sound as speech or a new request.
Delegation policy: NEVER GENERATE <|SpawnThinking|>, CHECK, ANSWER, SEARCH, OR USE TOOLS.",
        audience = field(&narration.audience),
        goal = field(&narration.goal),
    )
}

/// How slide `n` of `count` is asked for: its delivery, then its notes. A
/// slide without a delivery is read in the talk's tone.
pub fn request(n: usize, count: usize, delivery: Option<&str>, notes: &str) -> String {
    let delivery = delivery
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .unwrap_or("in the talk's tone.");
    format!(
        "Slide {n} of {count}. Delivery: {delivery}
Immediately read this passage aloud, word for word, then stop:

{notes}"
    )
}

/// An open live session.
pub struct Live {
    /// The writer's queue; taken when the session closes.
    out: Option<mpsc::UnboundedSender<Message>>,
    /// The writer, the reader and the silence feeder, in that order.
    tasks: Vec<JoinHandle<()>>,
}

impl Live {
    /// Opens a session told `instructions`; its voice goes to `heard`.
    pub async fn start(
        config: &Config,
        instructions: &str,
        heard: Arc<Mutex<Heard>>,
    ) -> Result<Self> {
        let (socket, _) = async_tungstenite::tokio::connect_async(config.request()?)
            .await
            .with_context(|| format!("connecting to GPT-Live at {}", config.url()))?;
        let (mut sink, mut stream) = socket.split();
        let (out, mut outgoing) = mpsc::unbounded_channel::<Message>();
        let writer = tokio::spawn(async move {
            while let Some(message) = outgoing.recv().await {
                if sink.send(message).await.is_err() {
                    break;
                }
            }
            sink.close(None).await.ok();
        });
        let (started, ready) = oneshot::channel::<Result<(), String>>();
        let reader = tokio::spawn(async move {
            let mut started = Some(started);
            while let Some(message) = stream.next().await {
                let text = match message {
                    Ok(Message::Text(text)) => text,
                    Ok(Message::Close(_)) | Err(_) => break,
                    Ok(_) => continue,
                };
                let Ok(event) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                match event["type"].as_str() {
                    Some("session.started") => {
                        if let Some(started) = started.take() {
                            started.send(Ok(())).ok();
                        }
                    }
                    Some("session.output_audio.delta") => {
                        let Some(bytes) = event["delta"]
                            .as_str()
                            .and_then(|delta| STANDARD.decode(delta).ok())
                        else {
                            continue;
                        };
                        let pcm: Vec<i16> = bytes
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|b| i16::from_le_bytes(*b))
                            .collect();
                        heard.lock().unwrap().take(&pcm);
                    }
                    Some("error") => {
                        let error = event["error"]["message"]
                            .as_str()
                            .map_or_else(|| event["error"].to_string(), str::to_string);
                        let error = format!("GPT-Live: {error}");
                        match started.take() {
                            Some(started) => {
                                started.send(Err(error)).ok();
                            }
                            None => heard.lock().unwrap().fail(error),
                        }
                    }
                    _ => {}
                }
            }
            heard
                .lock()
                .unwrap()
                .fail("GPT-Live closed the session".to_string());
        });
        let mut live = Self {
            out: Some(out.clone()),
            tasks: vec![writer, reader],
        };
        live.send(json!({
            "type": "session.start",
            "event_id": "start",
            "session": {
                "model": config.model,
                "instructions": instructions,
                "audio": {
                    "format": { "type": "audio/pcm", "rate": RATE },
                    "output": { "voice": VOICE },
                },
                "delegation": { "type": "client" },
            },
        }));
        match tokio::time::timeout(Duration::from_secs(30), ready).await {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(error))) => bail!(error),
            Ok(Err(_)) => bail!("GPT-Live closed the session before it started"),
            Err(_) => bail!("GPT-Live did not start a session within 30 seconds"),
        }
        // GPT-Live needs a continuous input stream: 20 ms of silence every
        // 20 ms.
        let feed = out;
        let silence = STANDARD.encode(vec![0u8; RATE / 50 * 2]);
        live.tasks.push(tokio::spawn(async move {
            let mut every = tokio::time::interval(Duration::from_millis(20));
            loop {
                every.tick().await;
                let event = json!({ "type": "session.input_audio.append", "audio": silence });
                if feed.send(Message::text(event.to_string())).is_err() {
                    break;
                }
            }
        }));
        Ok(live)
    }

    fn send(&self, event: Value) {
        if let Some(out) = &self.out {
            out.send(Message::text(event.to_string())).ok();
        }
    }

    /// Sends slide `n`'s [`request`].
    pub fn say(&self, n: usize, request: &str) {
        self.send(json!({
            "type": "session.instructions.append",
            "event_id": format!("slide_{n}"),
            "delegation_id": null,
            "content": request,
        }));
    }

    /// Ends the session.
    pub async fn close(mut self) {
        // The feeder is the last task: stop it, say goodbye, and let the
        // writer send what is queued and close the socket.
        if let Some(feeder) = self.tasks.pop() {
            feeder.abort();
            feeder.await.ok();
        }
        self.send(json!({ "type": "session.close", "event_id": "close" }));
        self.out.take();
        let writer = self.tasks.remove(0);
        tokio::time::timeout(Duration::from_secs(5), writer)
            .await
            .ok();
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(vars: &[(&str, &str)]) -> Result<Config> {
        Config::from_vars(|name| {
            vars.iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| v.to_string())
        })
    }

    fn error(vars: &[(&str, &str)]) -> String {
        config(vars).err().unwrap().to_string()
    }

    #[test]
    fn no_provider_is_an_error_naming_the_variables() {
        let message = error(&[]);
        for name in [OPENAI_API_KEY, AZURE_ENDPOINT, AZURE_API_KEY] {
            assert!(message.contains(name), "{message}");
        }
        // Blank counts as not set, and OpenAI's own variable is never read.
        assert_eq!(error(&[(OPENAI_API_KEY, " ")]), message);
        assert_eq!(error(&[("OPENAI_API_KEY", "sk-x")]), message);
    }

    #[test]
    fn the_openai_key_selects_openai_with_the_default_model() {
        let config = config(&[(OPENAI_API_KEY, "sk-test")]).unwrap();
        assert!(
            config.provider
                == Provider::OpenAi {
                    key: "sk-test".into()
                }
        );
        assert_eq!(config.model, DEFAULT_MODEL);
        assert_eq!(config.url(), "wss://api.openai.com/v1/live/sessions");
        let request = config.request().unwrap();
        assert_eq!(request.uri(), "wss://api.openai.com/v1/live/sessions");
        assert_eq!(request.headers()["authorization"], "Bearer sk-test");
        assert!(request.headers()["authorization"].is_sensitive());
        assert!(!request.headers().contains_key("api-key"));
    }

    #[test]
    fn the_azure_endpoint_selects_azure_even_beside_an_openai_key() {
        let config = config(&[
            (OPENAI_API_KEY, "sk-test"),
            (AZURE_ENDPOINT, "https://talks.openai.azure.com/"),
            (AZURE_API_KEY, "az-test"),
            (MODEL, "narrator"),
        ])
        .unwrap();
        assert!(
            config.provider
                == Provider::Azure {
                    host: "talks.openai.azure.com".into(),
                    key: "az-test".into()
                }
        );
        assert_eq!(config.model, "narrator");
        let request = config.request().unwrap();
        assert_eq!(
            request.uri(),
            "wss://talks.openai.azure.com/openai/v1/live/sessions"
        );
        assert_eq!(request.headers()["api-key"], "az-test");
        assert!(!request.headers().contains_key("authorization"));
    }

    #[test]
    fn an_azure_endpoint_may_carry_a_path_or_be_a_bare_host() {
        for endpoint in [
            "https://talks.openai.azure.com/openai/v1/",
            "talks.openai.azure.com",
            "wss://talks.openai.azure.com",
        ] {
            let config = config(&[(AZURE_ENDPOINT, endpoint), (AZURE_API_KEY, "k")]).unwrap();
            assert_eq!(
                config.url(),
                "wss://talks.openai.azure.com/openai/v1/live/sessions",
                "{endpoint}"
            );
        }
        assert!(
            error(&[(AZURE_ENDPOINT, "https://"), (AZURE_API_KEY, "k")]).contains("not an address")
        );
    }

    #[test]
    fn azure_without_its_key_is_an_error() {
        assert_eq!(
            error(&[
                (AZURE_ENDPOINT, "https://talks.openai.azure.com"),
                (OPENAI_API_KEY, "sk-test")
            ]),
            format!("{AZURE_ENDPOINT} is set but {AZURE_API_KEY} is not")
        );
    }

    /// The template as the making-a-video page shows it, its deck fields as
    /// `{placeholders}`.
    fn template() -> String {
        instructions(
            "{title}",
            0,
            &Narration {
                audience: Some("{audience}".to_string()),
                goal: Some("{goal}".to_string()),
                tone: Some("{tone}".to_string()),
                pronounce: vec!["{pronounce, one line each}".to_string()],
            },
        )
        .replace(
            "a recorded talk of 0 slides",
            "a recorded talk of {count} slides",
        )
    }

    #[test]
    fn making_a_video_shows_exactly_what_the_narrator_is_told() {
        let page = include_str!("../../../docs/boxes/viewer/making-a-video/README.md");
        assert!(
            page.contains(&template()),
            "the instructions on the page drifted from the code:\n{}",
            template()
        );
        let request = request(0, 0, Some("{delivery}"), "{notes}")
            .replace("Slide 0 of 0", "Slide {n} of {count}");
        assert!(
            page.contains(&request),
            "the per-slide request on the page drifted from the code:\n{request}"
        );
        assert!(page.contains(DEFAULT_TONE));
    }

    #[test]
    fn the_narrator_is_told_the_talk_and_the_tone_defaults() {
        let told = instructions(
            "Launch",
            3,
            &Narration {
                audience: Some("Engineers.".to_string()),
                goal: Some("They try it.".to_string()),
                tone: None,
                pronounce: vec![],
            },
        );
        assert!(
            told.starts_with("You are the narrator of \"Launch\", a recorded talk of 3 slides.")
        );
        assert!(told.contains("\nAudience: Engineers.\nGoal: They try it.\nTone: "));
        assert!(told.contains(DEFAULT_TONE));
        assert!(told.contains("Lean into the words that carry each slide's point.\n\nEach slide"));
        assert_eq!(
            request(2, 3, None, "Hello."),
            "Slide 2 of 3. Delivery: in the talk's tone.\nImmediately read this passage aloud, word for word, then stop:\n\nHello."
        );
    }
}

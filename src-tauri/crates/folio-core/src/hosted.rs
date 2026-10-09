//! Optional online generation through Groq (ADR 0017).
//!
//! This is the only provider that leaves the device, and only when the user
//! turned it on with their own key. It writes summaries and answers through
//! the same [`GenerationProvider`] boundary as the local runtime, so grounding,
//! citation checks and the untrusted-passage prompt rules are unchanged.
//! Embeddings, request interpretation and collection names never use it.

use crate::contracts::{GenerationOrigin, ProviderErrorCode};
use crate::error::{CoreError, CoreResult, NativeProviderErrorError};
use crate::generation::{
    parse_json_text, ChatMessage, GenerationBudget, GenerationProvider, GENERATION_TIMEOUT,
    MAX_OUTPUT_TOKENS,
};
use reqwest::blocking::{Client, RequestBuilder};
use serde_json::{json, Value};
use std::fmt;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

pub const GROQ_BASE_URL: &str = "https://api.groq.com/openai/v1";
/// Groq models that enforce a strict JSON schema, which Folio's citation
/// checks rely on. Other Groq models only promise valid JSON.
pub const GROQ_MODELS: [&str; 3] = [
    "openai/gpt-oss-20b",
    "openai/gpt-oss-120b",
    "qwen/qwen3.8-27b",
];
pub const DEFAULT_GROQ_MODEL: &str = GROQ_MODELS[0];
/// Hosted models aren't pinned to a revision Folio can verify.
pub const HOSTED_REVISION: &str = "hosted";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const CANCEL_POLL: Duration = Duration::from_millis(100);
/// gpt-oss reasons before it answers, and those tokens count toward the
/// completion limit; this keeps a short answer from being cut off.
const REASONING_HEADROOM: usize = 1024;
const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;
const MAX_KEY_CHARS: usize = 256;

/// A Groq API key. It is never printed, serialized or put in an error.
pub struct ApiKey(String);

impl ApiKey {
    pub fn new(key: &str) -> CoreResult<Self> {
        let key = key.trim();
        if key.is_empty()
            || key.chars().count() > MAX_KEY_CHARS
            || key.chars().any(|c| c.is_whitespace() || c.is_control())
        {
            return Err(CoreError::Message(
                "That doesn't look like a Groq API key.".into(),
            ));
        }
        Ok(Self(key.into()))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey([redacted])")
    }
}

pub struct GroqProvider {
    client: Client,
    base_url: String,
    key: ApiKey,
    model_id: String,
    active: AtomicBool,
}

impl fmt::Debug for GroqProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GroqProvider")
            .field("base_url", &self.base_url)
            .field("key", &self.key)
            .field("model_id", &self.model_id)
            .finish()
    }
}

pub fn is_supported_model(model_id: &str) -> bool {
    GROQ_MODELS.contains(&model_id)
}

impl GroqProvider {
    pub fn new(key: ApiKey, model_id: &str) -> CoreResult<Self> {
        let client = Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(GENERATION_TIMEOUT)
            .build()
            .map_err(|error| CoreError::Message(format!("HTTP client setup failed: {error}")))?;
        Self::with_client(client, GROQ_BASE_URL.into(), key, model_id)
    }

    fn with_client(
        client: Client,
        base_url: String,
        key: ApiKey,
        model_id: &str,
    ) -> CoreResult<Self> {
        if !is_supported_model(model_id) {
            return Err(CoreError::Message(format!(
                "Folio doesn't support the Groq model {model_id}."
            )));
        }
        Ok(Self {
            client,
            base_url,
            key,
            model_id: model_id.into(),
            active: AtomicBool::new(false),
        })
    }

    /// Ask Groq whether it accepts the key, without generating anything.
    pub fn check_key(&self, cancel: &AtomicBool) -> CoreResult<()> {
        let request = self
            .client
            .get(format!("{}/models", self.base_url))
            .bearer_auth(self.key.expose());
        let (status, body) = self.send(request, cancel)?;
        if (200..300).contains(&status) {
            Ok(())
        } else {
            Err(self.status_error(status, &body))
        }
    }

    /// Runs the request on a worker thread so a cancel returns at once; the
    /// abandoned request ends by itself within the client timeout.
    fn send(&self, request: RequestBuilder, cancel: &AtomicBool) -> CoreResult<(u16, String)> {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = request
                .send()
                .map_err(|error| transport_error(&error))
                .and_then(|response| {
                    let status = response.status().as_u16();
                    let mut body = String::new();
                    response
                        .take(MAX_RESPONSE_BYTES)
                        .read_to_string(&mut body)
                        .map(|_| (status, body))
                        .map_err(|_| unavailable())
                });
            let _ = sender.send(result);
        });
        let started = Instant::now();
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(provider(ProviderErrorCode::Cancelled, "Generation cancelled."));
            }
            match receiver.recv_timeout(CANCEL_POLL) {
                Ok(reply) => return reply,
                Err(RecvTimeoutError::Timeout) if started.elapsed() > GENERATION_TIMEOUT => {
                    return Err(timed_out());
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return Err(unavailable()),
            }
        }
    }

    fn status_error(&self, status: u16, body: &str) -> CoreError {
        // Groq doesn't echo keys, but a detail must never be the place one leaks.
        let detail = body
            .replace(self.key.expose(), "[redacted]")
            .chars()
            .take(500)
            .collect::<String>();
        let (code, message) = match status {
            401 | 403 => (
                ProviderErrorCode::OnlineKeyRejected,
                "Groq refused the saved key.",
            ),
            429 | 498 | 503 => (
                ProviderErrorCode::OnlineRateLimited,
                "Groq is busy or the key's limit was reached.",
            ),
            413 => (
                ProviderErrorCode::ContextLimit,
                "The request is too long for Groq.",
            ),
            400 if body.to_ascii_lowercase().contains("context") => (
                ProviderErrorCode::ContextLimit,
                "The request is too long for the Groq model.",
            ),
            400 | 422 => (
                ProviderErrorCode::InvalidModelOutput,
                "Groq couldn't write output in the shape Folio needs.",
            ),
            404 | 500..=599 => (
                ProviderErrorCode::OnlineUnavailable,
                "Groq couldn't answer right now.",
            ),
            _ => (
                ProviderErrorCode::InvalidModelOutput,
                "Groq returned an unexpected reply.",
            ),
        };
        CoreError::Provider(NativeProviderErrorError::new(code, message).with_detail(detail))
    }
}

impl GenerationProvider for GroqProvider {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn revision(&self) -> &str {
        HOSTED_REVISION
    }

    fn generate_json(
        &self,
        schema: &Value,
        messages: &[ChatMessage],
        budget: &GenerationBudget,
        cancel: &AtomicBool,
    ) -> CoreResult<Value> {
        if self
            .active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(provider(
                ProviderErrorCode::GenerationBusy,
                "Another generation request is active.",
            ));
        }
        let _active = ActiveGuard(&self.active);
        let request = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(self.key.expose())
            .json(&chat_payload(&self.model_id, schema, messages, budget));
        let (status, body) = self.send(request, cancel)?;
        if !(200..300).contains(&status) {
            return Err(self.status_error(status, &body));
        }
        let reply: Value = serde_json::from_str(&body).map_err(|_| {
            provider(
                ProviderErrorCode::InvalidModelOutput,
                "Groq's reply wasn't valid JSON.",
            )
        })?;
        if reply.pointer("/choices/0/finish_reason").and_then(Value::as_str) == Some("length") {
            return Err(provider(
                ProviderErrorCode::InvalidModelOutput,
                "Groq's output was cut off before it finished.",
            ));
        }
        let content = reply
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                provider(
                    ProviderErrorCode::InvalidModelOutput,
                    "Groq's reply had no output.",
                )
            })?;
        parse_json_text(content)
    }

    fn unload(&self) -> CoreResult<()> {
        Ok(())
    }

    fn origin(&self) -> GenerationOrigin {
        GenerationOrigin::Groq
    }
}

/// The chat request. Structured outputs can't be streamed on Groq, so the
/// whole reply arrives at once. `cache_prompt` is a llama.cpp setting and is
/// not sent.
fn chat_payload(
    model_id: &str,
    schema: &Value,
    messages: &[ChatMessage],
    budget: &GenerationBudget,
) -> Value {
    let output_tokens = budget.max_output_tokens.min(MAX_OUTPUT_TOKENS);
    let mut payload = json!({
        "model": model_id,
        "messages": messages,
        "stream": false,
        "temperature": budget.temperature,
        "seed": budget.seed,
        "max_completion_tokens": output_tokens,
        "response_format": {
            "type": "json_schema",
            "json_schema": {"name": "folio_output", "strict": true, "schema": schema}
        }
    });
    if model_id.starts_with("openai/gpt-oss") {
        payload["reasoning_effort"] = json!("low");
        payload["include_reasoning"] = json!(false);
        payload["max_completion_tokens"] = json!(output_tokens + REASONING_HEADROOM);
    } else if model_id.starts_with("qwen/") {
        // Matches the local Qwen setup, which runs with thinking off.
        payload["reasoning_effort"] = json!("none");
    }
    payload
}

struct ActiveGuard<'a>(&'a AtomicBool);

impl Drop for ActiveGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn provider(code: ProviderErrorCode, message: impl Into<String>) -> CoreError {
    CoreError::Provider(NativeProviderErrorError::new(code, message))
}

fn transport_error(error: &reqwest::Error) -> CoreError {
    if error.is_timeout() {
        timed_out()
    } else {
        unavailable()
    }
}

fn timed_out() -> CoreError {
    provider(ProviderErrorCode::TimedOut, "Groq took too long to answer.")
}

fn unavailable() -> CoreError {
    provider(
        ProviderErrorCode::OnlineUnavailable,
        "Folio couldn't reach Groq. Check your internet connection.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;

    const KEY: &str = "test-key-not-real-0123456789";

    struct Reply {
        status: u16,
        body: String,
        delay: Duration,
    }

    fn reply(status: u16, body: Value) -> Reply {
        Reply {
            status,
            body: body.to_string(),
            delay: Duration::ZERO,
        }
    }

    /// A loopback server that answers each connection with the next reply and
    /// hands back each raw request it read.
    fn serve(replies: Vec<Reply>) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            for reply in replies {
                let Ok((stream, _)) = listener.accept() else { return };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse().unwrap();
                    }
                    request.push_str(&line);
                    if line == "\r\n" {
                        break;
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                request.push_str(&String::from_utf8(body).unwrap());
                let _ = sender.send(request);
                thread::sleep(reply.delay);
                let mut stream = stream;
                let _ = write!(
                    stream,
                    "HTTP/1.1 {} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    reply.status,
                    reply.body.len(),
                    reply.body
                );
            }
        });
        (base, receiver)
    }

    fn groq(base: String, model: &str) -> GroqProvider {
        let client = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        GroqProvider::with_client(client, base, ApiKey::new(KEY).unwrap(), model).unwrap()
    }

    fn completion(content: Value) -> Value {
        json!({"choices": [{"message": {"content": content.to_string()}, "finish_reason": "stop"}]})
    }

    fn messages(text: &str) -> Vec<ChatMessage> {
        vec![ChatMessage {
            role: "user".into(),
            content: text.into(),
        }]
    }

    fn schema() -> Value {
        json!({"type": "object", "properties": {"name": {"type": ["string", "null"]}},
               "required": ["name"], "additionalProperties": false})
    }

    fn code(error: CoreError) -> ProviderErrorCode {
        match error {
            CoreError::Provider(error) => error.code,
            other => panic!("expected a provider error, got {other}"),
        }
    }

    #[test]
    fn sends_a_strict_schema_request_and_reads_the_output() {
        let (base, requests) = serve(vec![reply(200, completion(json!({"name": "Plano"})))]);
        let provider = groq(base, "openai/gpt-oss-20b");
        let output = provider
            .generate_json(
                &schema(),
                &messages("Palitan sa project plan ang deadline, pls"),
                &GenerationBudget::default(),
                &AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(output, json!({"name": "Plano"}));

        let request = requests.recv().unwrap();
        assert!(request.starts_with("POST /chat/completions "));
        assert!(request.contains(&format!("authorization: Bearer {KEY}"))
            || request.contains(&format!("Authorization: Bearer {KEY}")));
        let body: Value = serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["model"], "openai/gpt-oss-20b");
        assert_eq!(body["stream"], false);
        assert_eq!(body["response_format"]["type"], "json_schema");
        assert_eq!(body["response_format"]["json_schema"]["strict"], true);
        assert_eq!(body["response_format"]["json_schema"]["schema"], schema());
        assert_eq!(body["reasoning_effort"], "low");
        assert_eq!(body["include_reasoning"], false);
        assert_eq!(
            body["max_completion_tokens"],
            MAX_OUTPUT_TOKENS + REASONING_HEADROOM
        );
        assert_eq!(
            body["messages"][0]["content"],
            "Palitan sa project plan ang deadline, pls"
        );
        assert!(body.get("cache_prompt").is_none());
        assert!(body.get("chat_template_kwargs").is_none());
    }

    #[test]
    fn only_gpt_oss_gets_reasoning_headroom() {
        let budget = GenerationBudget {
            max_output_tokens: 64,
            ..GenerationBudget::default()
        };
        let qwen = chat_payload("qwen/qwen3.8-27b", &schema(), &messages("Hi"), &budget);
        assert_eq!(qwen["max_completion_tokens"], 64);
        assert_eq!(qwen["reasoning_effort"], "none");
        assert!(qwen.get("include_reasoning").is_none());
    }

    #[test]
    fn statuses_map_to_actionable_errors() {
        for (status, expected) in [
            (401, ProviderErrorCode::OnlineKeyRejected),
            (403, ProviderErrorCode::OnlineKeyRejected),
            (429, ProviderErrorCode::OnlineRateLimited),
            (498, ProviderErrorCode::OnlineRateLimited),
            (503, ProviderErrorCode::OnlineRateLimited),
            (413, ProviderErrorCode::ContextLimit),
            (422, ProviderErrorCode::InvalidModelOutput),
            (500, ProviderErrorCode::OnlineUnavailable),
            (404, ProviderErrorCode::OnlineUnavailable),
        ] {
            let (base, _requests) = serve(vec![reply(
                status,
                json!({"error": {"message": format!("echo {KEY}")}}),
            )]);
            let error = groq(base, DEFAULT_GROQ_MODEL)
                .generate_json(
                    &schema(),
                    &messages("Hello"),
                    &GenerationBudget::default(),
                    &AtomicBool::new(false),
                )
                .unwrap_err();
            let CoreError::Provider(error) = error else {
                panic!("expected a provider error")
            };
            assert_eq!(error.code, expected, "HTTP {status}");
            assert!(!error.detail.unwrap_or_default().contains(KEY));
            assert!(!error.message.contains(KEY));
        }
    }

    #[test]
    fn a_cut_off_or_malformed_reply_is_invalid_output() {
        let cut_off = json!({"choices": [{"message": {"content": "{\"na"}, "finish_reason": "length"}]});
        let (base, _requests) = serve(vec![
            reply(200, cut_off),
            reply(200, json!({"choices": []})),
        ]);
        let provider = groq(base, DEFAULT_GROQ_MODEL);
        for _ in 0..2 {
            let error = provider
                .generate_json(
                    &schema(),
                    &messages("Hello"),
                    &GenerationBudget::default(),
                    &AtomicBool::new(false),
                )
                .unwrap_err();
            assert_eq!(code(error), ProviderErrorCode::InvalidModelOutput);
        }
    }

    #[test]
    fn cancelling_returns_without_waiting_for_groq() {
        let (base, _requests) = serve(vec![Reply {
            delay: Duration::from_secs(5),
            ..reply(200, completion(json!({"name": "late"})))
        }]);
        let provider = groq(base, DEFAULT_GROQ_MODEL);
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(200));
            flag.store(true, Ordering::Release);
        });
        let started = Instant::now();
        let error = provider
            .generate_json(&schema(), &messages("Hi"), &GenerationBudget::default(), &cancel)
            .unwrap_err();
        assert_eq!(code(error), ProviderErrorCode::Cancelled);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn an_unreachable_service_is_reported_as_unavailable() {
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let error = groq(format!("http://127.0.0.1:{port}"), DEFAULT_GROQ_MODEL)
            .check_key(&AtomicBool::new(false))
            .unwrap_err();
        assert_eq!(code(error), ProviderErrorCode::OnlineUnavailable);
    }

    #[test]
    fn checking_a_key_asks_for_the_model_list() {
        let (base, requests) = serve(vec![
            reply(200, json!({"data": []})),
            reply(401, json!({"error": {"message": "Invalid API Key"}})),
        ]);
        let provider = groq(base, DEFAULT_GROQ_MODEL);
        provider.check_key(&AtomicBool::new(false)).unwrap();
        assert!(requests.recv().unwrap().starts_with("GET /models "));
        let error = provider.check_key(&AtomicBool::new(false)).unwrap_err();
        assert_eq!(code(error), ProviderErrorCode::OnlineKeyRejected);
    }

    #[test]
    fn the_key_is_never_printed() {
        let provider = groq("http://127.0.0.1:9".into(), DEFAULT_GROQ_MODEL);
        assert!(!format!("{provider:?}").contains(KEY));
        assert!(!format!("{:?}", ApiKey::new(KEY).unwrap()).contains(KEY));
    }

    #[test]
    fn keys_and_models_are_checked_before_any_request() {
        assert!(ApiKey::new("  ").is_err());
        assert!(ApiKey::new("two words").is_err());
        assert_eq!(ApiKey::new(&format!(" {KEY}\n")).unwrap().expose(), KEY);
        assert!(GroqProvider::new(ApiKey::new(KEY).unwrap(), "llama-3.1-8b-instant").is_err());
        assert!(GroqProvider::new(ApiKey::new(KEY).unwrap(), DEFAULT_GROQ_MODEL).is_ok());
    }

    #[test]
    fn the_production_client_refuses_plain_http() {
        let provider = GroqProvider::new(ApiKey::new(KEY).unwrap(), DEFAULT_GROQ_MODEL).unwrap();
        let provider = GroqProvider {
            base_url: "http://127.0.0.1:9".into(),
            ..provider
        };
        let error = provider.check_key(&AtomicBool::new(false)).unwrap_err();
        assert_eq!(code(error), ProviderErrorCode::OnlineUnavailable);
    }
}

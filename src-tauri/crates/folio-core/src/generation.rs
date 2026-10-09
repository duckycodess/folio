use crate::contracts::ProviderErrorCode;
use crate::error::{CoreError, CoreResult, NativeProviderErrorError};
use crate::models::VerifiedModelFile;
use reqwest::blocking::Client;
use serde::Serialize;
use serde_json::{json, Value};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

pub const N_CTX: usize = 4096;
pub const MAX_OUTPUT_TOKENS: usize = 512;
pub const MAX_PASSAGES: usize = 8;
pub const MAX_PASSAGE_CHARS: usize = 12_000;
/// Total time budget for one generation request, enforced across the whole
/// streamed response (the HTTP client's timeout applies per read only).
pub const GENERATION_TIMEOUT: Duration = Duration::from_secs(120);
/// Longest caller-supplied text (a question or an instruction) put into a
/// prompt. Longer input is refused as a context limit before generation.
pub const MAX_REQUEST_CHARS: usize = 2_000;
pub const IDLE_UNLOAD: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Debug, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Clone, Debug)]
pub struct GenerationBudget {
    pub max_output_tokens: usize,
    pub temperature: f32,
    pub seed: i64,
}

impl Default for GenerationBudget {
    fn default() -> Self {
        Self {
            max_output_tokens: MAX_OUTPUT_TOKENS,
            temperature: 0.0,
            seed: 7,
        }
    }
}

pub trait GenerationProvider: Send + Sync {
    fn model_id(&self) -> &str;
    fn revision(&self) -> &str;
    fn generate_json(
        &self,
        schema: &Value,
        messages: &[ChatMessage],
        budget: &GenerationBudget,
        cancel: &AtomicBool,
    ) -> CoreResult<Value>;
    fn unload(&self) -> CoreResult<()>;
}

struct RunningServer {
    child: Child,
    port: u16,
    api_key: String,
    last_used: Instant,
    /// Closing this handle (including when Folio itself dies) kills the child.
    #[cfg(windows)]
    _job: Option<job::KillOnClose>,
}

#[derive(Default)]
struct ServerState {
    running: Option<RunningServer>,
}

pub struct LlamaServerProvider {
    executable: PathBuf,
    model: VerifiedModelFile,
    threads: usize,
    state: Arc<Mutex<ServerState>>,
    active: Arc<AtomicBool>,
    client: Client,
    idle_unload: Duration,
    reaper_stop: Arc<AtomicBool>,
    reaper: Option<thread::JoinHandle<()>>,
}

impl LlamaServerProvider {
    pub fn from_verified_model(
        executable: impl Into<PathBuf>,
        model: VerifiedModelFile,
        threads: usize,
    ) -> CoreResult<Self> {
        Self::from_verified_model_with_idle(executable, model, threads, IDLE_UNLOAD)
    }

    fn from_verified_model_with_idle(
        executable: impl Into<PathBuf>,
        model: VerifiedModelFile,
        threads: usize,
        idle_unload: Duration,
    ) -> CoreResult<Self> {
        let executable = executable.into();
        if !executable.is_file() {
            return Err(provider(
                ProviderErrorCode::RuntimeMissing,
                "The llama.cpp server executable is not installed.",
            ));
        }
        if !model.path.is_file() {
            return Err(provider(
                ProviderErrorCode::ModelCorrupt,
                "The verified generation model file is unavailable.",
            ));
        }
        let client = loopback_client(GENERATION_TIMEOUT)?;
        let state = Arc::new(Mutex::new(ServerState::default()));
        let active = Arc::new(AtomicBool::new(false));
        let reaper_stop = Arc::new(AtomicBool::new(false));
        let reaper = Some(spawn_idle_reaper(
            state.clone(),
            active.clone(),
            reaper_stop.clone(),
            idle_unload,
        ));
        Ok(Self {
            executable,
            model,
            threads: threads.max(1),
            state,
            active,
            client,
            idle_unload,
            reaper_stop,
            reaper,
        })
    }

    /// Fixed launch arguments. The per-process key is passed as a file (not
    /// on the command line, where other local users could read it), and the
    /// `/slots` endpoint is disabled so it cannot expose prompt state.
    pub fn build_server_args(
        executable: &Path,
        model_path: &Path,
        port: u16,
        api_key_file: &Path,
        threads: usize,
    ) -> Vec<String> {
        vec![
            executable.to_string_lossy().into_owned(),
            "--host".into(),
            "127.0.0.1".into(),
            "--port".into(),
            port.to_string(),
            "--api-key-file".into(),
            api_key_file.to_string_lossy().into_owned(),
            "--no-slots".into(),
            "-m".into(),
            model_path.to_string_lossy().into_owned(),
            "-c".into(),
            N_CTX.to_string(),
            "-np".into(),
            "1".into(),
            "--threads".into(),
            threads.max(1).to_string(),
            "--jinja".into(),
            "--no-webui".into(),
        ]
    }

    fn endpoint(&self, cancel: &AtomicBool) -> CoreResult<(String, String)> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| CoreError::Message("Generation runtime state is unavailable.".into()))?;
        if let Some(running) = state.running.as_mut() {
            if running.child.try_wait().ok().flatten().is_some() {
                state.running = None;
            } else if running.last_used.elapsed() < self.idle_unload {
                running.last_used = Instant::now();
                return Ok((loopback_url(running.port)?, running.api_key.clone()));
            } else {
                let _ = running.child.kill();
                let _ = running.child.wait();
                state.running = None;
            }
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(provider(
                ProviderErrorCode::Cancelled,
                "Generation cancelled.",
            ));
        }
        let port = TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
        let api_key = Uuid::new_v4().simple().to_string();
        let key_file = write_api_key_file(&api_key)?;
        let args = Self::build_server_args(
            &self.executable,
            &self.model.path,
            port,
            &key_file,
            self.threads,
        );
        let mut command = Command::new(&self.executable);
        command
            .args(args.iter().skip(1))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: no console window for each model load.
            command.creation_flags(0x0800_0000);
        }
        let child = command.spawn().map_err(|error| {
            let _ = fs::remove_file(&key_file);
            CoreError::Provider(NativeProviderErrorError::new(
                ProviderErrorCode::RuntimeStartFailed,
                format!("Could not start llama.cpp: {error}"),
            ))
        })?;
        state.running = Some(RunningServer {
            #[cfg(windows)]
            _job: job::KillOnClose::assign(&child).ok(),
            child,
            port,
            api_key: api_key.clone(),
            last_used: Instant::now(),
        });
        drop(state);
        let url = loopback_url(port)?;
        let deadline = Instant::now() + GENERATION_TIMEOUT.min(Duration::from_secs(30));
        let ready = loop {
            if cancel.load(Ordering::Relaxed) {
                let _ = self.unload();
                break Err(provider(
                    ProviderErrorCode::Cancelled,
                    "Generation cancelled.",
                ));
            }
            // A child that exits during startup (for example because the port
            // was taken) fails now, before any request carries the key.
            if let Some(status) = self.exited_child_status() {
                let _ = self.unload();
                break Err(provider(
                    ProviderErrorCode::RuntimeStartFailed,
                    format!("llama.cpp exited during startup ({status})."),
                ));
            }
            // /health needs no key, so a port squatter never receives it here.
            if self
                .client
                .get(format!("{url}/health"))
                .send()
                .is_ok_and(|response| response.status().is_success())
                && self.exited_child_status().is_none()
            {
                break Ok((url.clone(), api_key.clone()));
            }
            if Instant::now() >= deadline {
                let _ = self.unload();
                break Err(provider(
                    ProviderErrorCode::RuntimeStartFailed,
                    "llama.cpp did not become ready before the startup timeout.",
                ));
            }
            thread::sleep(Duration::from_millis(100));
        };
        // The server has read the key by the time it answers /health.
        let _ = fs::remove_file(&key_file);
        ready
    }

    fn exited_child_status(&self) -> Option<String> {
        let mut state = self.state.lock().ok()?;
        let running = state.running.as_mut()?;
        running
            .child
            .try_wait()
            .ok()
            .flatten()
            .map(|status| status.to_string())
    }

    pub fn cancel_active(&self) -> CoreResult<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| CoreError::Message("Generation runtime state is unavailable.".into()))?;
        if let Some(mut running) = state.running.take() {
            let _ = running.child.kill();
            let _ = running.child.wait();
        }
        Ok(())
    }
}

impl GenerationProvider for LlamaServerProvider {
    fn model_id(&self) -> &str {
        &self.model.descriptor.id
    }

    fn revision(&self) -> &str {
        &self.model.descriptor.revision
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
                "Another local generation request is active.",
            ));
        }
        let _active = ActiveGuard(&self.active);
        let (base_url, api_key) = self.endpoint(cancel)?;
        let payload = json!({
            "model": self.model.descriptor.id,
            "messages": messages,
            "stream": true,
            "temperature": budget.temperature,
            "seed": budget.seed,
            "max_tokens": budget.max_output_tokens.min(MAX_OUTPUT_TOKENS),
            "response_format": {
                "type": "json_schema",
                "json_schema": { "name": "folio_output", "strict": true, "schema": schema }
            },
            "chat_template_kwargs": { "enable_thinking": false }
        });
        let started = Instant::now();
        let mut response = self
            .client
            .post(format!("{base_url}/v1/chat/completions"))
            .header("Authorization", format!("Bearer {api_key}"))
            .json(&payload)
            .send()
            .map_err(|error| {
                if cancel.load(Ordering::Relaxed) {
                    provider(ProviderErrorCode::Cancelled, "Generation cancelled.")
                } else {
                    CoreError::Provider(NativeProviderErrorError::new(
                        ProviderErrorCode::RuntimeStartFailed,
                        format!("Generation request failed: {error}"),
                    ))
                }
            })?;
        if cancel.load(Ordering::Relaxed) {
            return Err(provider(
                ProviderErrorCode::Cancelled,
                "Generation cancelled.",
            ));
        }
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(runtime_status_error(status.as_u16(), &body));
        }
        let mut reader = BufReader::new(&mut response);
        let mut generated = String::new();
        let mut line = String::new();
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(provider(
                    ProviderErrorCode::Cancelled,
                    "Generation cancelled.",
                ));
            }
            if started.elapsed() > GENERATION_TIMEOUT {
                drop(reader);
                let _ = self.unload();
                return Err(provider(
                    ProviderErrorCode::TimedOut,
                    "Local generation exceeded its time limit.",
                ));
            }
            line.clear();
            let bytes = match reader.read_line(&mut line) {
                Ok(bytes) => bytes,
                Err(_error) if cancel.load(Ordering::Relaxed) => {
                    return Err(provider(
                        ProviderErrorCode::Cancelled,
                        "Generation cancelled.",
                    ));
                }
                Err(error) => return Err(CoreError::Io(error)),
            };
            if bytes == 0 {
                break;
            }
            let data = line.trim().strip_prefix("data:").map(str::trim);
            let Some(data) = data else { continue };
            if data == "[DONE]" {
                break;
            }
            let event: Value = serde_json::from_str(data).map_err(|error| {
                CoreError::Provider(NativeProviderErrorError::new(
                    ProviderErrorCode::InvalidModelOutput,
                    format!("Invalid streaming JSON: {error}"),
                ))
            })?;
            if let Some(content) = event
                .pointer("/choices/0/delta/content")
                .and_then(Value::as_str)
            {
                generated.push_str(content);
            }
            if let Some(content) = event
                .pointer("/choices/0/message/content")
                .and_then(Value::as_str)
            {
                generated.push_str(content);
            }
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(provider(
                ProviderErrorCode::Cancelled,
                "Generation cancelled.",
            ));
        }
        parse_json_text(&generated)
    }

    fn unload(&self) -> CoreResult<()> {
        self.cancel_active()
    }
}

impl Drop for LlamaServerProvider {
    fn drop(&mut self) {
        self.reaper_stop.store(true, Ordering::Release);
        if let Some(reaper) = self.reaper.take() {
            let _ = reaper.join();
        }
        let _ = self.unload();
    }
}

fn spawn_idle_reaper(
    state: Arc<Mutex<ServerState>>,
    active: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    idle_unload: Duration,
) -> thread::JoinHandle<()> {
    let interval = idle_unload
        .min(Duration::from_millis(100))
        .max(Duration::from_millis(10));
    thread::spawn(move || {
        while !stop.load(Ordering::Acquire) {
            thread::sleep(interval);
            if stop.load(Ordering::Acquire) || active.load(Ordering::Acquire) {
                continue;
            }
            let Ok(mut guard) = state.lock() else {
                break;
            };
            let expired = guard
                .running
                .as_ref()
                .is_some_and(|running| running.last_used.elapsed() >= idle_unload);
            if expired {
                if let Some(mut running) = guard.running.take() {
                    let _ = running.child.kill();
                    let _ = running.child.wait();
                }
            }
        }
    })
}

struct ActiveGuard<'a>(&'a AtomicBool);

impl Drop for ActiveGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// HTTP client for the local inference server only. System and environment
/// proxies are ignored: reqwest honours `HTTP_PROXY` even for `127.0.0.1`,
/// which would send prompts, retrieved passages and the per-process key to a
/// proxy instead of the loopback server.
pub fn loopback_client(timeout: Duration) -> CoreResult<Client> {
    Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(2))
        .timeout(timeout)
        .build()
        .map_err(|error| CoreError::Message(format!("HTTP client setup failed: {error}")))
}

/// Map a non-success llama.cpp response. An over-context request is a
/// context limit; other bodies are kept out of the message (the boundary
/// reports only a digest of the detail).
fn runtime_status_error(status: u16, body: &str) -> CoreError {
    let lowered = body.to_ascii_lowercase();
    if (400..500).contains(&status) && lowered.contains("context") {
        return provider(
            ProviderErrorCode::ContextLimit,
            "The request is longer than the local model's context window.",
        );
    }
    CoreError::Provider(
        NativeProviderErrorError::new(
            ProviderErrorCode::InvalidModelOutput,
            format!("The local runtime returned HTTP {status}."),
        )
        .with_detail(body.chars().take(500).collect::<String>()),
    )
}

/// Refuse caller text longer than `MAX_REQUEST_CHARS` before it reaches a
/// prompt.
pub fn check_request_length(text: &str) -> CoreResult<()> {
    if text.chars().count() > MAX_REQUEST_CHARS {
        return Err(provider(
            ProviderErrorCode::ContextLimit,
            format!("Requests are limited to {MAX_REQUEST_CHARS} characters."),
        ));
    }
    Ok(())
}

/// Write the per-process key to a new file readable only by this user.
fn write_api_key_file(api_key: &str) -> CoreResult<PathBuf> {
    let path = std::env::temp_dir().join(format!("folio-llama-key-{}", Uuid::new_v4().simple()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path)?;
    file.write_all(api_key.as_bytes())?;
    file.sync_all()?;
    Ok(path)
}

#[cfg(windows)]
mod job {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    /// A Windows Job Object that kills its process when the handle closes,
    /// including when Folio exits or crashes.
    pub struct KillOnClose(HANDLE);

    // The handle is only closed, never shared across threads concurrently.
    unsafe impl Send for KillOnClose {}

    impl KillOnClose {
        pub fn assign(child: &std::process::Child) -> std::io::Result<Self> {
            unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if job.is_null() {
                    return Err(std::io::Error::last_os_error());
                }
                let job = Self(job);
                let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                if SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    &limits as *const _ as *const core::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                ) == 0
                {
                    return Err(std::io::Error::last_os_error());
                }
                if AssignProcessToJobObject(job.0, child.as_raw_handle() as HANDLE) == 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(job)
            }
        }
    }

    impl Drop for KillOnClose {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

pub fn loopback_url(port: u16) -> CoreResult<String> {
    if port == 0 {
        return Err(CoreError::Message("A loopback port is required.".into()));
    }
    Ok(format!("http://127.0.0.1:{port}"))
}

pub fn validate_loopback_host(host: &str) -> CoreResult<()> {
    if host != "127.0.0.1" {
        return Err(CoreError::Message(
            "Local inference endpoints must bind to 127.0.0.1.".into(),
        ));
    }
    Ok(())
}

pub fn parse_json_text(text: &str) -> CoreResult<Value> {
    let trimmed = text.trim();
    let trimmed = trimmed
        .strip_prefix("\x60\x60\x60json")
        .and_then(|value| value.strip_suffix("\x60\x60\x60"))
        .map(str::trim)
        .unwrap_or(trimmed);
    serde_json::from_str(trimmed).map_err(|error| {
        CoreError::Provider(NativeProviderErrorError::new(
            ProviderErrorCode::InvalidModelOutput,
            format!("Model output was not valid JSON: {error}"),
        ))
    })
}

fn provider(code: ProviderErrorCode, message: impl Into<String>) -> CoreError {
    CoreError::Provider(NativeProviderErrorError::new(code, message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn over_context_responses_and_long_requests_are_context_limits() {
        let over = runtime_status_error(
            400,
            "{\"error\":{\"message\":\"the request exceeds the available context size\"}}",
        );
        assert!(matches!(
            over,
            CoreError::Provider(ref failure) if failure.code == ProviderErrorCode::ContextLimit
        ));
        let other = runtime_status_error(500, "secret prompt text");
        match other {
            CoreError::Provider(failure) => {
                assert_eq!(failure.code, ProviderErrorCode::InvalidModelOutput);
                assert!(!failure.message.contains("secret prompt text"));
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(check_request_length(&"a".repeat(MAX_REQUEST_CHARS)).is_ok());
        assert!(check_request_length(&"ñ".repeat(MAX_REQUEST_CHARS + 1)).is_err());
    }

    #[test]
    fn only_loopback_host_is_accepted() {
        assert!(validate_loopback_host("127.0.0.1").is_ok());
        assert!(validate_loopback_host("localhost").is_err());
        assert!(validate_loopback_host("0.0.0.0").is_err());
    }

    #[test]
    fn runtime_arguments_are_fixed_and_loopback_only() {
        let args = LlamaServerProvider::build_server_args(
            Path::new("/opt/llama-server"),
            Path::new("/data/models/qwen.gguf"),
            43210,
            Path::new("/tmp/folio-llama-key-test"),
            3,
        );
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "--host" && pair[1] == "127.0.0.1"));
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "--api-key-file" && pair[1] == "/tmp/folio-llama-key-test"));
        assert!(!args.iter().any(|arg| arg == "--api-key"));
        assert!(args.iter().any(|arg| arg == "--no-slots"));
        assert!(args.iter().any(|arg| arg == "--jinja"));
        assert!(!args.iter().any(|arg| arg.contains("sh -c")));
    }

    #[test]
    fn api_key_files_are_private_and_hold_only_the_key() {
        let path = write_api_key_file("per-process-key").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "per-process-key");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn fenced_json_is_parsed_but_invalid_json_is_rejected() {
        assert_eq!(
            parse_json_text("\x60\x60\x60json\n{\"ok\":true}\n\x60\x60\x60").unwrap()["ok"],
            true
        );
        assert!(parse_json_text("not json").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn idle_reaper_unloads_an_expired_server() {
        use crate::contracts::{ModelDescriptor, ModelRole};
        use crate::models::VerifiedModelFile;
        use std::fs;
        use std::process::Command;

        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("llama-server");
        let model_path = temp.path().join("model.gguf");
        fs::write(&executable, b"runtime").unwrap();
        fs::write(&model_path, b"model").unwrap();
        let provider = LlamaServerProvider::from_verified_model_with_idle(
            &executable,
            VerifiedModelFile {
                descriptor: ModelDescriptor {
                    id: "test-generation".into(),
                    role: ModelRole::Generation,
                    repo: "local/test".into(),
                    revision: "test".into(),
                    files: Vec::new(),
                    quantization: "test".into(),
                    license: "test".into(),
                    runtime: "test".into(),
                    optional_pack: false,
                },
                path: model_path,
            },
            1,
            Duration::from_millis(20),
        )
        .unwrap();
        let child = Command::new("sleep").arg("5").spawn().unwrap();
        provider.state.lock().unwrap().running = Some(RunningServer {
            #[cfg(windows)]
            _job: None,
            child,
            port: 1,
            api_key: "test".into(),
            last_used: Instant::now() - Duration::from_secs(1),
        });

        let mut unloaded = false;
        for _ in 0..50 {
            if provider.state.lock().unwrap().running.is_none() {
                unloaded = true;
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(unloaded, "idle server was not reaped");
    }

    #[cfg(unix)]
    #[test]
    fn cancel_active_stops_the_owned_server() {
        use crate::contracts::{ModelDescriptor, ModelRole};
        use crate::models::VerifiedModelFile;
        use std::fs;
        use std::process::Command;

        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("llama-server");
        let model_path = temp.path().join("model.gguf");
        fs::write(&executable, b"runtime").unwrap();
        fs::write(&model_path, b"model").unwrap();
        let provider = LlamaServerProvider::from_verified_model_with_idle(
            &executable,
            VerifiedModelFile {
                descriptor: ModelDescriptor {
                    id: "test-generation".into(),
                    role: ModelRole::Generation,
                    repo: "local/test".into(),
                    revision: "test".into(),
                    files: Vec::new(),
                    quantization: "test".into(),
                    license: "test".into(),
                    runtime: "test".into(),
                    optional_pack: false,
                },
                path: model_path,
            },
            1,
            Duration::from_secs(60),
        )
        .unwrap();
        let child = Command::new("sleep").arg("5").spawn().unwrap();
        provider.state.lock().unwrap().running = Some(RunningServer {
            #[cfg(windows)]
            _job: None,
            child,
            port: 1,
            api_key: "test".into(),
            last_used: Instant::now(),
        });

        provider.cancel_active().unwrap();
        assert!(provider.state.lock().unwrap().running.is_none());
    }

    #[test]
    #[ignore = "requires verified FOLIO_LLAMA_SERVER and FOLIO_QWEN_MODEL files"]
    fn real_llama_server_smoke_uses_the_local_runtime() {
        use crate::contracts::{ModelDescriptor, ModelFile, ModelRole};
        use crate::models::VerifiedModelFile;
        use std::fs;
        use std::sync::atomic::AtomicBool;

        let executable = std::env::var("FOLIO_LLAMA_SERVER").expect("FOLIO_LLAMA_SERVER");
        let model_path = std::env::var("FOLIO_QWEN_MODEL").expect("FOLIO_QWEN_MODEL");
        let model_path_buf = PathBuf::from(&model_path);
        let descriptor = ModelDescriptor {
            id: "qwen3-0.6b-q4-k-m".into(),
            role: ModelRole::Generation,
            repo: "unsloth/Qwen3-0.6B-GGUF".into(),
            revision: "50968a4468ef4233ed78cd7c3de230dd1d61a56b".into(),
            files: vec![ModelFile {
                path: model_path_buf
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("model.gguf")
                    .into(),
                sha256: "a".repeat(64),
                bytes: fs::metadata(&model_path_buf).expect("model metadata").len(),
                download_url: None,
            }],
            quantization: "Q4_K_M".into(),
            license: "apache-2.0".into(),
            runtime: "llama.cpp".into(),
            optional_pack: false,
        };
        let provider = LlamaServerProvider::from_verified_model(
            executable,
            VerifiedModelFile {
                descriptor,
                path: model_path_buf,
            },
            2,
        )
        .expect("local llama provider loads");
        let output = provider
            .generate_json(
                &json!({
                    "type": "object",
                    "additionalProperties": false,
                    "properties": { "answer": { "type": "string" } },
                    "required": ["answer"]
                }),
                &[ChatMessage {
                    role: "user".into(),
                    content: "Return JSON with answer equal to exactly ok.".into(),
                }],
                &GenerationBudget::default(),
                &AtomicBool::new(false),
            )
            .expect("local llama provider generates");
        assert!(output.get("answer").and_then(Value::as_str).is_some());
        provider.unload().expect("local llama provider unloads");
    }
}

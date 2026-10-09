//! The sequential Model Lab runner.
//!
//! One embedding model measures retrieval; each requested generation model then
//! runs, strictly one at a time, against a fresh disposable copy of the corpus.
//! For every case the process is restarted, the case runs once (the first
//! request after a restart: `cold`), and then runs again at once on the same
//! process (the immediate repeat). Startup time is recorded apart from task
//! time, and the operating system's file cache is never controlled. Nothing is
//! graded by a model: outcomes come from [`crate::lab::checks`], and a summary
//! is left for a person to review.

use crate::chunking::{Chunk, ChunkSource, InterimTextChunker, TextDocument};
use crate::contracts::{DocumentRecord, EmbeddingSpace, ProviderErrorCode};
use crate::embeddings::{
    markdown_title, passage_embedding_texts, EmbeddingKind, EmbeddingProvider, DEFAULT_MAX_TOKENS,
};
use crate::error::{CoreError, CoreResult};
use crate::generation::{
    ChatMessage, GenerationBudget, GenerationProvider, LlamaServerProvider, MAX_OUTPUT_TOKENS,
    MAX_PASSAGES, N_CTX,
};
use crate::grounding::{detect_language, summarize_document, summary_passages};
use crate::interpretation::interpret_request_traced;
use crate::lab::checks::{
    check_edit, check_interpretation, check_retrieval, check_summary, Evaluation, RETRIEVAL_LIMIT,
};
use crate::lab::host::{hardware_summary, host_info, parse_backend_log, prompt_fingerprint};
use crate::lab::memory::PeakReading;
use crate::lab::record::{
    ApplyOutcome, BenchmarkRecord, BenchmarkTask, Check, Conditions, HostInfo, MemoryEntry,
    MemoryProcess, ModelRef, Observation, PageCache, RequestPosition, RunStatus, RunSummary,
    RuntimeDetail, SchemaVersion, ServerSettings, StartupWarmup, Timing,
};
use crate::lab::sink::LabSink;
use crate::lab::suite::{Corpus, Suite, SuiteCase};
use crate::lab::workspace::{LabWorkspace, LabWorkspaces, Snapshot};
use crate::models::sha256_bytes;
use crate::retrieval::HybridRetriever;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// A generation model the lab can restart on demand.
pub trait LabGenerator: GenerationProvider {
    /// Stops any running server, starts a fresh one and returns once it is
    /// ready to serve, without sending a request. Returns its process id.
    fn restart(&self, cancel: &AtomicBool) -> CoreResult<Option<u32>>;
    /// The running server's process id, if one is running.
    fn server_pid(&self) -> Option<u32>;
    /// The server's own output for the current process, if the lab captured it.
    fn server_log(&self) -> Option<String> {
        None
    }
}

impl LabGenerator for LlamaServerProvider {
    fn restart(&self, cancel: &AtomicBool) -> CoreResult<Option<u32>> {
        GenerationProvider::unload(self)?;
        self.ensure_started(cancel)?;
        Ok(LlamaServerProvider::server_pid(self))
    }

    fn server_pid(&self) -> Option<u32> {
        LlamaServerProvider::server_pid(self)
    }

    fn server_log(&self) -> Option<String> {
        self.lab_log()
    }
}

/// The runtime detail for a record, with the backend facts the server itself
/// printed when the lab captured its output.
fn observed_runtime(base: &RuntimeDetail, log: Option<&str>) -> RuntimeDetail {
    let mut runtime = base.clone();
    if let (Some(backend), Some(log)) = (runtime.backend.as_mut(), log) {
        let seen = parse_backend_log(log);
        backend.observed_log_excerpt = seen.excerpt;
        backend.gpu_layers_offloaded = seen.gpu_layers_offloaded;
        backend.layers_total = seen.layers_total;
    }
    runtime
}

/// Reads process peaks. Real runs use the operating system; tests script it.
pub trait MemoryProbe {
    fn process_peak(&self, pid: u32) -> PeakReading;
    fn self_peak(&self) -> PeakReading;
}

pub struct OsMemoryProbe;

impl MemoryProbe for OsMemoryProbe {
    fn process_peak(&self, pid: u32) -> PeakReading {
        crate::lab::memory::process_peak(pid)
    }

    fn self_peak(&self) -> PeakReading {
        crate::lab::memory::self_peak()
    }
}

/// An opened generation model and what a record must say about it.
pub struct GeneratorHandle {
    pub model: ModelRef,
    pub runtime: RuntimeDetail,
    pub generator: Box<dyn LabGenerator>,
}

/// Opens one generation model at a time. The runner drops the previous handle
/// before opening the next, so only one server can be alive.
pub trait GeneratorFactory {
    fn open(&self, model_id: &str) -> CoreResult<GeneratorHandle>;
}

pub struct EmbeddingSubject<'a> {
    pub model: ModelRef,
    pub runtime: RuntimeDetail,
    pub provider: &'a dyn EmbeddingProvider,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LabProgress {
    pub run_id: String,
    pub step: String,
    pub case_id: Option<String>,
    pub model_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunEnd {
    Completed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Flow {
    Continue,
    Cancelled,
}

pub fn system_clock_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

fn is_cancelled(error: &CoreError) -> bool {
    matches!(error, CoreError::Provider(failure) if failure.code == ProviderErrorCode::Cancelled)
}

/// Sends every request with prompt reuse off and counts them, so an identical
/// repeat is processed in full. Only Model Lab requests are changed.
struct LabProvider<'a> {
    inner: &'a dyn LabGenerator,
    requests: AtomicUsize,
}

impl<'a> LabProvider<'a> {
    fn new(inner: &'a dyn LabGenerator) -> Self {
        Self {
            inner,
            requests: AtomicUsize::new(0),
        }
    }

    fn take_requests(&self) -> u32 {
        self.requests.swap(0, Ordering::AcqRel) as u32
    }
}

impl GenerationProvider for LabProvider<'_> {
    fn model_id(&self) -> &str {
        self.inner.model_id()
    }

    fn revision(&self) -> &str {
        self.inner.revision()
    }

    fn generate_json(
        &self,
        schema: &Value,
        messages: &[ChatMessage],
        budget: &GenerationBudget,
        cancel: &AtomicBool,
    ) -> CoreResult<Value> {
        self.requests.fetch_add(1, Ordering::AcqRel);
        let mut lab_budget = budget.clone();
        lab_budget.cache_prompt = Some(false);
        self.inner
            .generate_json(schema, messages, &lab_budget, cancel)
    }

    fn unload(&self) -> CoreResult<()> {
        self.inner.unload()
    }
}

struct Loaded {
    documents: Vec<DocumentRecord>,
    contents: HashMap<String, String>,
    chunks: Vec<Chunk>,
}

struct Measured {
    case_id: String,
    task: BenchmarkTask,
    model: ModelRef,
    runtime: RuntimeDetail,
    prompt_sha256: String,
    conditions: Conditions,
    server_settings: Option<ServerSettings>,
    timing: Timing,
    task_duration_ms: u64,
    evaluation: Evaluation,
    memory: Vec<MemoryEntry>,
    output: Value,
}

pub struct LabRunner<'a> {
    pub run_id: String,
    pub workspaces: &'a LabWorkspaces,
    pub suite: &'a Suite,
    pub corpus: &'a Corpus,
    pub embedding: EmbeddingSubject<'a>,
    /// Generation models in the order they run.
    pub generation_model_ids: Vec<String>,
    pub factory: &'a dyn GeneratorFactory,
    pub probe: &'a dyn MemoryProbe,
    pub sink: &'a mut dyn LabSink,
    pub cancel: &'a AtomicBool,
    pub threads: u32,
    pub host: HostInfo,
    pub clock_ms: fn() -> u64,
    pub progress: Option<&'a dyn Fn(&LabProgress)>,
}

impl LabRunner<'_> {
    pub fn run(&mut self) -> CoreResult<RunEnd> {
        let mut requested = vec![self.embedding.model.id.clone()];
        requested.extend(self.generation_model_ids.iter().cloned());
        let mut summary = RunSummary {
            run_id: self.run_id.clone(),
            status: RunStatus::Running,
            requested_model_ids: requested,
            suite: self.suite.reference(),
            corpus_sha256: self.corpus.sha256.clone(),
            host: self.host.clone(),
            server_settings: ServerSettings {
                startup_warmup: StartupWarmup::DefaultOn,
                cache_prompt: false,
            },
            started_at: (self.clock_ms)(),
            ended_at: None,
            index_build_ms: None,
            error: None,
            schema_version: SchemaVersion,
        };
        self.sink.run_status(&summary)?;
        let result = self.execute(&mut summary);
        let _ = self.workspaces.remove_run(&self.run_id);
        summary.ended_at = Some((self.clock_ms)());
        match &result {
            Ok(RunEnd::Completed) => summary.status = RunStatus::Completed,
            Ok(RunEnd::Cancelled) => summary.status = RunStatus::Cancelled,
            Err(error) => {
                summary.status = RunStatus::Failed;
                summary.error = Some(error.to_string());
            }
        }
        match result {
            Ok(end) => {
                self.sink.run_status(&summary)?;
                Ok(end)
            }
            Err(error) => {
                let _ = self.sink.run_status(&summary);
                Err(error)
            }
        }
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn announce(&self, step: &str, case_id: Option<&str>, model_id: Option<&str>) {
        if let Some(progress) = self.progress {
            progress(&LabProgress {
                run_id: self.run_id.clone(),
                step: step.to_string(),
                case_id: case_id.map(str::to_string),
                model_id: model_id.map(str::to_string),
            });
        }
    }

    fn execute(&mut self, summary: &mut RunSummary) -> CoreResult<RunEnd> {
        if self.retrieval_phase(summary)? == Flow::Cancelled {
            return Ok(RunEnd::Cancelled);
        }
        for model_id in self.generation_model_ids.clone() {
            if self.cancelled() || self.generation_phase(&model_id)? == Flow::Cancelled {
                return Ok(RunEnd::Cancelled);
            }
        }
        Ok(RunEnd::Completed)
    }

    fn load_workspace(&self, workspace: &LabWorkspace) -> CoreResult<Loaded> {
        let mut sources = Vec::new();
        let mut contents = HashMap::new();
        for document in &self.corpus.documents {
            let path = workspace.root().join(&document.relative_path);
            let content = fs::read_to_string(&path)?;
            let name = Path::new(&document.relative_path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&document.relative_path)
                .to_string();
            let record = DocumentRecord {
                id: document.relative_path.clone(),
                workspace_id: "model-lab".into(),
                relative_path: document.relative_path.clone(),
                title: markdown_title(&name, &content),
                name,
                language: detect_language(&content),
                media_type: "text/markdown".into(),
                size_bytes: content.len() as u64,
                modified_at_ms: None,
                content: Some(content.clone()),
                content_hash: None,
            };
            contents.insert(document.relative_path.clone(), content.clone());
            sources.push(TextDocument::new(record, content));
        }
        let chunker = InterimTextChunker::new(sources);
        Ok(Loaded {
            documents: chunker.documents(),
            chunks: chunker.all_chunks()?,
            contents,
        })
    }

    fn base_conditions(&self) -> Conditions {
        let budget = GenerationBudget::default();
        Conditions {
            n_ctx: N_CTX as u32,
            max_output_tokens: MAX_OUTPUT_TOKENS as u32,
            max_passages: MAX_PASSAGES as u32,
            temperature: budget.temperature,
            seed: budget.seed,
            threads: self.threads,
            corpus_sha256: self.corpus.sha256.clone(),
            page_cache: PageCache::NotControlled,
        }
    }

    fn emit(&mut self, measured: Measured) -> CoreResult<()> {
        let embedding_model_id = self.embedding.model.id.clone();
        let wanted = match measured.model.role {
            crate::contracts::ModelRole::Embedding => MemoryProcess::Folio,
            crate::contracts::ModelRole::Generation => MemoryProcess::LlamaServer,
        };
        let peak = measured
            .memory
            .iter()
            .find(|entry| entry.process == wanted)
            .and_then(|entry| entry.peak_bytes);
        let model_file_bytes: u64 = measured.model.files.iter().map(|file| file.bytes).sum();
        let record = BenchmarkRecord {
            case_id: measured.case_id,
            task: measured.task,
            model_id: measured.model.id.clone(),
            revision: measured.model.revision.clone(),
            quantization: measured.model.quantization.clone(),
            runtime: measured.runtime.version.clone(),
            hardware: hardware_summary(&self.host),
            context_tokens: measured.conditions.n_ctx,
            cold: measured.timing.request_position
                == RequestPosition::FirstRequestAfterServerRestart,
            task_duration_ms: measured.task_duration_ms,
            correctness: measured.evaluation.correctness,
            peak_process_ram_bytes: peak,
            model_disk_bytes: model_file_bytes,
            id: uuid::Uuid::new_v4().to_string(),
            run_id: self.run_id.clone(),
            created_at: (self.clock_ms)(),
            suite: self.suite.reference(),
            prompt_sha256: measured.prompt_sha256,
            model: measured.model,
            embedding_model_id,
            runtime_detail: measured.runtime,
            host: self.host.clone(),
            conditions: measured.conditions,
            timing: measured.timing,
            server_settings: measured.server_settings,
            observation: Observation::SinglePair,
            memory: measured.memory,
            model_file_bytes,
            objective_checks: measured.evaluation.checks,
            output_sha256: sha256_bytes(
                &serde_json::to_vec(&measured.output).map_err(CoreError::from)?,
            ),
            output: measured.output,
            reviews: Vec::new(),
            schema_version: SchemaVersion,
            apply: ApplyOutcome::not_run(),
        };
        self.sink.record(&record)
    }

    fn failure(error: &CoreError) -> (Evaluation, Value) {
        let code = match error {
            CoreError::Provider(failure) => Some(format!("{:?}", failure.code)),
            _ => None,
        };
        (
            Evaluation {
                correctness: None,
                checks: vec![Check {
                    name: "completed".into(),
                    passed: Some(false),
                    detail: error.to_string(),
                }],
            },
            json!({ "error": error.to_string(), "providerCode": code }),
        )
    }

    fn retrieval_phase(&mut self, summary: &mut RunSummary) -> CoreResult<Flow> {
        let workspace = self.workspaces.create(&self.run_id, self.corpus)?;
        let expected = LabWorkspaces::expected_snapshot(self.corpus);
        let loaded = self.load_workspace(&workspace)?;

        self.announce("indexing", None, Some(&self.embedding.model.id.clone()));
        let started = Instant::now();
        let texts = passage_embedding_texts(&loaded.documents, &loaded.chunks);
        let vectors =
            match self
                .embedding
                .provider
                .embed(&texts, EmbeddingKind::Passage, Some(self.cancel))
            {
                Err(error) if is_cancelled(&error) => return Ok(Flow::Cancelled),
                other => other?,
            };
        let space: EmbeddingSpace = self.embedding.provider.space().clone();
        let mut retriever = HybridRetriever::default();
        retriever
            .vector_index
            .replace(space, loaded.chunks.clone(), vectors)?;
        summary.index_build_ms = Some(started.elapsed().as_millis() as u64);
        self.sink.run_status(summary)?;

        let cases: Vec<SuiteCase> = self
            .suite
            .cases
            .iter()
            .filter(|case| matches!(case, SuiteCase::Retrieval { .. }))
            .cloned()
            .collect();
        for case in &cases {
            if self.cancelled() {
                return Ok(Flow::Cancelled);
            }
            self.announce("retrieval", Some(case.id()), None);
            if self.retrieval_case(case, &loaded, &retriever)? == Flow::Cancelled {
                return Ok(Flow::Cancelled);
            }
        }
        self.workspaces.verify_unchanged(&workspace, &expected)?;
        self.workspaces.remove(&workspace)?;
        Ok(Flow::Continue)
    }

    fn retrieval_case(
        &mut self,
        case: &SuiteCase,
        loaded: &Loaded,
        retriever: &HybridRetriever,
    ) -> CoreResult<Flow> {
        let SuiteCase::Retrieval { id, input, .. } = case else {
            return Ok(Flow::Continue);
        };
        for (pass, position) in [
            RequestPosition::FirstRequestAfterServerRestart,
            RequestPosition::ImmediateRepeat,
        ]
        .into_iter()
        .enumerate()
        {
            if pass == 0 {
                // The session reloads on the first embed, so its load is part
                // of this first request's time (processStartMs stays null).
                self.embedding.provider.unload()?;
            }
            let started = Instant::now();
            let attempt = self
                .embedding
                .provider
                .embed_query(input, Some(self.cancel))
                .and_then(|query| {
                    let results = retriever.search(
                        &loaded.documents,
                        &loaded.chunks,
                        input,
                        Some(&query),
                        RETRIEVAL_LIMIT,
                    )?;
                    Ok((query, results))
                });
            let task_duration_ms = started.elapsed().as_millis() as u64;
            let (evaluation, output) = match attempt {
                Err(error) if is_cancelled(&error) => return Ok(Flow::Cancelled),
                Err(error) => Self::failure(&error),
                Ok((query, results)) => {
                    let gate = retriever.evidence_gate(&query)?;
                    let ranked: Vec<Value> = results
                        .iter()
                        .map(|result| {
                            json!({
                                "documentId": result.document.id,
                                "relativePath": result.document.relative_path,
                                "score": result.score,
                                "method": result.method,
                            })
                        })
                        .collect();
                    (
                        check_retrieval(case, &results),
                        json!({
                            "query": input,
                            "rankedDocuments": ranked,
                            "evidenceGate": gate,
                            "sessionLoadIncludedInTask": pass == 0,
                        }),
                    )
                }
            };
            let memory = self.probe.self_peak().into_entry(
                MemoryProcess::Folio,
                Some(std::process::id()),
                "Folio process lifetime peak, read after this request; includes the app and all earlier work in the process",
            );
            let space = self.embedding.provider.space().clone();
            let measured = Measured {
                case_id: id.clone(),
                task: BenchmarkTask::Retrieval,
                model: self.embedding.model.clone(),
                runtime: self.embedding.runtime.clone(),
                prompt_sha256: sha256_bytes(space.preprocessing_fingerprint.as_bytes()),
                conditions: Conditions {
                    n_ctx: DEFAULT_MAX_TOKENS as u32,
                    max_output_tokens: 0,
                    max_passages: RETRIEVAL_LIMIT as u32,
                    temperature: 0.0,
                    seed: 0,
                    ..self.base_conditions()
                },
                server_settings: None,
                timing: Timing {
                    process_start_ms: None,
                    requests_in_task: 1,
                    requests_since_process_start: pass as u32,
                    request_position: position,
                },
                task_duration_ms,
                evaluation,
                memory: vec![memory],
                output,
            };
            self.emit(measured)?;
        }
        Ok(Flow::Continue)
    }

    fn generation_phase(&mut self, model_id: &str) -> CoreResult<Flow> {
        self.announce("starting", None, Some(model_id));
        let workspace = self.workspaces.create(&self.run_id, self.corpus)?;
        let expected = LabWorkspaces::expected_snapshot(self.corpus);
        let loaded = self.load_workspace(&workspace)?;
        let handle = self.factory.open(model_id)?;
        let lab = LabProvider::new(&*handle.generator);

        let cases: Vec<SuiteCase> = self
            .suite
            .cases
            .iter()
            .filter(|case| !matches!(case, SuiteCase::Retrieval { .. }))
            .cloned()
            .collect();
        for case in &cases {
            if self.cancelled() {
                return Ok(Flow::Cancelled);
            }
            self.announce("case", Some(case.id()), Some(model_id));
            let flow = self.generation_case(case, &handle, &lab, &loaded, &workspace, &expected)?;
            if flow == Flow::Cancelled {
                return Ok(Flow::Cancelled);
            }
        }
        drop(lab);
        handle.generator.unload()?;
        drop(handle);
        self.workspaces.verify_unchanged(&workspace, &expected)?;
        self.workspaces.remove(&workspace)?;
        Ok(Flow::Continue)
    }

    fn generation_case(
        &mut self,
        case: &SuiteCase,
        handle: &GeneratorHandle,
        lab: &LabProvider<'_>,
        loaded: &Loaded,
        workspace: &LabWorkspace,
        expected: &Snapshot,
    ) -> CoreResult<Flow> {
        let task = match case {
            SuiteCase::Interpretation { .. } => BenchmarkTask::Interpretation,
            SuiteCase::Summary { .. } => BenchmarkTask::Summary,
            SuiteCase::Edit { .. } => BenchmarkTask::Edit,
            SuiteCase::Retrieval { .. } => return Ok(Flow::Continue),
        };
        let restart_started = Instant::now();
        match handle.generator.restart(self.cancel) {
            Err(error) if is_cancelled(&error) => return Ok(Flow::Cancelled),
            // A model that cannot start is a measurement: it is recorded for
            // this case and the run moves on to the next case and model.
            Err(error) => {
                return self.startup_failure(case, task, handle, &error, restart_started);
            }
            Ok(_) => {}
        };
        let process_start_ms = restart_started.elapsed().as_millis() as u64;

        let mut requests_before = 0_u32;
        for (pass, position) in [
            RequestPosition::FirstRequestAfterServerRestart,
            RequestPosition::ImmediateRepeat,
        ]
        .into_iter()
        .enumerate()
        {
            lab.take_requests();
            let started = Instant::now();
            let attempt = self.run_task(case, lab, loaded, workspace, expected);
            let task_duration_ms = started.elapsed().as_millis() as u64;
            let requests_in_task = lab.take_requests();
            let (evaluation, output) = match attempt {
                Err(error) if is_cancelled(&error) => return Ok(Flow::Cancelled),
                Err(error) => Self::failure(&error),
                Ok(done) => done,
            };
            let pid = handle.generator.server_pid();
            let reading = match pid {
                Some(pid) => self.probe.process_peak(pid),
                None => PeakReading {
                    peak_bytes: None,
                    method: "none".into(),
                    unavailable_reason: Some(
                        "no llama-server process was running when the peak was read".into(),
                    ),
                },
            };
            let scope = if pass == 0 {
                "llama-server process lifetime peak since its start; covers startup, model load and this request, the first after the restart"
            } else {
                "llama-server process lifetime peak since its start; covers startup, model load, the first request and this immediate repeat"
            };
            let measured = Measured {
                case_id: case.id().to_string(),
                task,
                model: handle.model.clone(),
                runtime: observed_runtime(
                    &handle.runtime,
                    handle.generator.server_log().as_deref(),
                ),
                prompt_sha256: prompt_fingerprint(),
                conditions: self.base_conditions(),
                server_settings: Some(ServerSettings {
                    startup_warmup: StartupWarmup::DefaultOn,
                    cache_prompt: false,
                }),
                timing: Timing {
                    process_start_ms: Some(process_start_ms),
                    requests_in_task,
                    requests_since_process_start: requests_before,
                    request_position: position,
                },
                task_duration_ms,
                evaluation,
                memory: vec![reading.into_entry(MemoryProcess::LlamaServer, pid, scope)],
                output,
            };
            self.emit(measured)?;
            requests_before += requests_in_task;
        }
        Ok(Flow::Continue)
    }

    /// Records that the server did not start for a case. No request was made,
    /// so nothing is graded and no correctness is claimed.
    fn startup_failure(
        &mut self,
        case: &SuiteCase,
        task: BenchmarkTask,
        handle: &GeneratorHandle,
        error: &CoreError,
        started: Instant,
    ) -> CoreResult<Flow> {
        let (evaluation, mut output) = Self::failure(error);
        output["stage"] = json!("startup");
        let evaluation = Evaluation {
            correctness: None,
            checks: vec![Check {
                name: "serverStarted".into(),
                passed: Some(false),
                detail: evaluation.checks[0].detail.clone(),
            }],
        };
        let reading = PeakReading {
            peak_bytes: None,
            method: "none".into(),
            unavailable_reason: Some("the server did not start, so no process was measured".into()),
        };
        let measured = Measured {
            case_id: case.id().to_string(),
            task,
            model: handle.model.clone(),
            runtime: observed_runtime(&handle.runtime, handle.generator.server_log().as_deref()),
            prompt_sha256: prompt_fingerprint(),
            conditions: self.base_conditions(),
            server_settings: Some(ServerSettings {
                startup_warmup: StartupWarmup::DefaultOn,
                cache_prompt: false,
            }),
            timing: Timing {
                process_start_ms: Some(started.elapsed().as_millis() as u64),
                requests_in_task: 0,
                requests_since_process_start: 0,
                request_position: RequestPosition::FirstRequestAfterServerRestart,
            },
            task_duration_ms: 0,
            evaluation,
            memory: vec![reading.into_entry(
                MemoryProcess::LlamaServer,
                None,
                "no server process was running",
            )],
            output,
        };
        self.emit(measured)?;
        Ok(Flow::Continue)
    }

    fn run_task(
        &self,
        case: &SuiteCase,
        lab: &LabProvider<'_>,
        loaded: &Loaded,
        workspace: &LabWorkspace,
        expected: &Snapshot,
    ) -> CoreResult<(Evaluation, Value)> {
        match case {
            SuiteCase::Interpretation { input, .. } => {
                let trace = interpret_request_traced(
                    lab,
                    input,
                    &loaded.documents,
                    &loaded.contents,
                    &loaded.chunks,
                    self.cancel,
                )?;
                Ok((
                    check_interpretation(case, &trace.result),
                    json!({
                        "request": input,
                        "result": trace.result,
                        "rawModelOutput": trace.raw_model_output,
                        "promptSha256": trace.prompt_sha256,
                    }),
                ))
            }
            SuiteCase::Edit { input, .. } => {
                let trace = interpret_request_traced(
                    lab,
                    input,
                    &loaded.documents,
                    &loaded.contents,
                    &loaded.chunks,
                    self.cancel,
                )?;
                let unchanged = self
                    .workspaces
                    .verify_unchanged(workspace, expected)
                    .is_ok();
                Ok((
                    check_edit(case, &trace.result, unchanged),
                    json!({
                        "request": input,
                        "result": trace.result,
                        "rawModelOutput": trace.raw_model_output,
                        "promptSha256": trace.prompt_sha256,
                        "workspaceUnchanged": unchanged,
                    }),
                ))
            }
            SuiteCase::Summary {
                input, document, ..
            } => {
                let record = loaded
                    .documents
                    .iter()
                    .find(|candidate| &candidate.id == document)
                    .ok_or_else(|| {
                        CoreError::Message(format!("{document} is not in the corpus"))
                    })?;
                let content = &loaded.contents[document];
                let supplied = summary_passages(
                    document,
                    content,
                    record.content_hash.as_deref().unwrap_or_default(),
                );
                let language = detect_language(input);
                let summary =
                    summarize_document(lab, supplied.clone(), language.clone(), self.cancel)?;
                Ok((
                    check_summary(case, &summary, &supplied),
                    json!({
                        "request": input,
                        "document": document,
                        "requestedLanguage": language,
                        "suppliedPassages": supplied,
                        "result": summary,
                        "detectedOutputLanguage": {
                            "value": detect_language(&summary.text),
                            "method": "heuristic lexical detector; observation only",
                        },
                        "review": "notReviewed",
                    }),
                ))
            }
            SuiteCase::Retrieval { .. } => Err(CoreError::Message(
                "a retrieval case is not a generation task".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lab::record::{ModelFileRef, RuntimeName};
    use crate::lab::sink::MemorySink;
    use std::sync::atomic::AtomicI64;
    use std::sync::Arc;

    struct ScriptedEmbedder {
        space: EmbeddingSpace,
        unloads: AtomicUsize,
    }

    impl ScriptedEmbedder {
        fn new() -> Self {
            Self {
                space: EmbeddingSpace {
                    model_id: "scripted-embedder".into(),
                    revision: "r".into(),
                    quantization: "none".into(),
                    dimensions: 32,
                    preprocessing_fingerprint: "scripted-prefixes".into(),
                },
                unloads: AtomicUsize::new(0),
            }
        }
    }

    impl EmbeddingProvider for ScriptedEmbedder {
        fn space(&self) -> &EmbeddingSpace {
            &self.space
        }

        fn embed(
            &self,
            texts: &[String],
            _kind: EmbeddingKind,
            _cancel: Option<&AtomicBool>,
        ) -> CoreResult<Vec<Vec<f32>>> {
            Ok(texts
                .iter()
                .map(|text| {
                    let mut vector = vec![0.0_f32; 32];
                    for word in text.to_lowercase().split_whitespace() {
                        let bucket = word.bytes().map(usize::from).sum::<usize>() % 32;
                        vector[bucket] += 1.0;
                    }
                    let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt().max(1.0);
                    vector.iter().map(|v| v / norm).collect()
                })
                .collect())
        }

        fn unload(&self) -> CoreResult<()> {
            self.unloads.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[derive(Default)]
    struct Live {
        now: AtomicI64,
        max: AtomicI64,
        opened: AtomicUsize,
    }

    struct ScriptedGenerator {
        id: String,
        live: Arc<Live>,
        restarts: AtomicUsize,
        requests: AtomicUsize,
        saw_cache_prompt_off: AtomicBool,
    }

    impl GenerationProvider for ScriptedGenerator {
        fn model_id(&self) -> &str {
            &self.id
        }

        fn revision(&self) -> &str {
            "scripted"
        }

        fn generate_json(
            &self,
            schema: &Value,
            _messages: &[ChatMessage],
            budget: &GenerationBudget,
            _cancel: &AtomicBool,
        ) -> CoreResult<Value> {
            self.requests.fetch_add(1, Ordering::SeqCst);
            if budget.cache_prompt == Some(false) {
                self.saw_cache_prompt_off.store(true, Ordering::SeqCst);
            }
            if schema["properties"].get("intent").is_some() {
                Ok(json!({
                    "intent": "edit",
                    "targetDescription": "project plan",
                    "find": "October 20",
                    "replace": "October 23",
                    "destination": null,
                    "newContent": null,
                    "clarification": null
                }))
            } else {
                Ok(json!({ "notes": [] }))
            }
        }

        fn unload(&self) -> CoreResult<()> {
            Ok(())
        }
    }

    impl LabGenerator for ScriptedGenerator {
        fn restart(&self, _cancel: &AtomicBool) -> CoreResult<Option<u32>> {
            self.restarts.fetch_add(1, Ordering::SeqCst);
            if self.id.starts_with("broken-") {
                return Err(CoreError::Provider(
                    crate::error::NativeProviderErrorError::new(
                        ProviderErrorCode::RuntimeStartFailed,
                        "scripted start failure: unsupported architecture",
                    ),
                ));
            }
            Ok(Some(4242))
        }

        fn server_pid(&self) -> Option<u32> {
            Some(4242)
        }

        fn server_log(&self) -> Option<String> {
            Some("load_tensors: offloaded 0/29 layers to GPU".into())
        }
    }

    impl Drop for ScriptedGenerator {
        fn drop(&mut self) {
            self.live.now.fetch_sub(1, Ordering::SeqCst);
        }
    }

    struct ScriptedFactory {
        live: Arc<Live>,
        order: std::sync::Mutex<Vec<String>>,
    }

    impl GeneratorFactory for ScriptedFactory {
        fn open(&self, model_id: &str) -> CoreResult<GeneratorHandle> {
            let now = self.live.now.fetch_add(1, Ordering::SeqCst) + 1;
            self.live.max.fetch_max(now, Ordering::SeqCst);
            self.live.opened.fetch_add(1, Ordering::SeqCst);
            self.order.lock().unwrap().push(model_id.to_string());
            Ok(GeneratorHandle {
                model: model_ref(model_id, crate::contracts::ModelRole::Generation, 1000),
                runtime: RuntimeDetail {
                    name: RuntimeName::LlamaCpp,
                    version: "scripted-llama".into(),
                    backend: None,
                },
                generator: Box::new(ScriptedGenerator {
                    id: model_id.to_string(),
                    live: self.live.clone(),
                    restarts: AtomicUsize::new(0),
                    requests: AtomicUsize::new(0),
                    saw_cache_prompt_off: AtomicBool::new(false),
                }),
            })
        }
    }

    struct FixedProbe;

    impl MemoryProbe for FixedProbe {
        fn process_peak(&self, _pid: u32) -> PeakReading {
            PeakReading {
                peak_bytes: Some(123_456),
                method: "scripted".into(),
                unavailable_reason: None,
            }
        }

        fn self_peak(&self) -> PeakReading {
            PeakReading {
                peak_bytes: None,
                method: "scripted".into(),
                unavailable_reason: Some("scripted: unavailable".into()),
            }
        }
    }

    fn model_ref(id: &str, role: crate::contracts::ModelRole, bytes: u64) -> ModelRef {
        ModelRef {
            id: id.into(),
            role,
            repo: "scripted/repo".into(),
            revision: "scripted-rev".into(),
            quantization: "scripted-q".into(),
            files: vec![ModelFileRef {
                path: "model.bin".into(),
                sha256: "0".repeat(64),
                bytes,
            }],
            // The scripted factory treats ids starting "candidate-" as evaluation candidates.
            catalog: if id.starts_with("candidate-") {
                crate::lab::record::ModelCatalog::EvaluationCandidate
            } else {
                crate::lab::record::ModelCatalog::Product
            },
            evaluation_only: id.starts_with("candidate-"),
            license: None,
            license_note: None,
        }
    }

    fn fixed_clock() -> u64 {
        1_760_000_000_000
    }

    struct Harness {
        dir: tempfile::TempDir,
        embedder: ScriptedEmbedder,
        live: Arc<Live>,
    }

    fn harness() -> Harness {
        Harness {
            dir: tempfile::tempdir().unwrap(),
            embedder: ScriptedEmbedder::new(),
            live: Arc::new(Live::default()),
        }
    }

    fn run_with(
        harness: &Harness,
        models: &[&str],
        cancel: &AtomicBool,
        sink: &mut MemorySink,
    ) -> (CoreResult<RunEnd>, Vec<String>) {
        let workspaces = LabWorkspaces::new(harness.dir.path());
        let suite = Suite::embedded().unwrap();
        let corpus = Corpus::embedded();
        let factory = ScriptedFactory {
            live: harness.live.clone(),
            order: std::sync::Mutex::new(Vec::new()),
        };
        let probe = FixedProbe;
        let result = LabRunner {
            run_id: "run-test".into(),
            workspaces: &workspaces,
            suite: &suite,
            corpus: &corpus,
            embedding: EmbeddingSubject {
                model: model_ref(
                    "scripted-embedder",
                    crate::contracts::ModelRole::Embedding,
                    500,
                ),
                runtime: RuntimeDetail {
                    name: RuntimeName::OnnxRuntime,
                    version: "scripted-ort".into(),
                    backend: None,
                },
                provider: &harness.embedder,
            },
            generation_model_ids: models.iter().map(|id| id.to_string()).collect(),
            factory: &factory,
            probe: &probe,
            sink,
            cancel,
            threads: 2,
            host: host_info(),
            clock_ms: fixed_clock,
            progress: None,
        }
        .run();
        let order = factory.order.lock().unwrap().clone();
        (result, order)
    }

    #[test]
    fn every_case_is_measured_cold_then_as_an_immediate_repeat() {
        let harness = harness();
        let mut sink = MemorySink::default();
        let cancel = AtomicBool::new(false);
        let (result, _) = run_with(&harness, &["model-a"], &cancel, &mut sink);
        assert_eq!(result.unwrap(), RunEnd::Completed);

        // 3 retrieval + 3 generation cases, two records each.
        assert_eq!(sink.records.len(), 12);
        let mut by_case: HashMap<(String, String), Vec<&BenchmarkRecord>> = HashMap::new();
        for record in &sink.records {
            by_case
                .entry((record.model_id.clone(), record.case_id.clone()))
                .or_default()
                .push(record);
        }
        assert_eq!(by_case.len(), 6);
        for ((_, case), records) in &by_case {
            assert_eq!(records.len(), 2, "{case}");
            assert!(records[0].cold && !records[1].cold, "{case}");
            assert_eq!(
                records[0].timing.request_position,
                RequestPosition::FirstRequestAfterServerRestart
            );
            assert_eq!(
                records[1].timing.request_position,
                RequestPosition::ImmediateRepeat
            );
            assert_eq!(records[0].timing.requests_since_process_start, 0);
            assert!(records[1].timing.requests_since_process_start >= 1);
            assert_eq!(records[0].observation, Observation::SinglePair);
        }
        let run = sink.runs.last().unwrap();
        assert_eq!(run.status, RunStatus::Completed);
        assert!(run.index_build_ms.is_some());
        assert_eq!(sink.runs.first().unwrap().status, RunStatus::Running);
    }

    #[test]
    fn the_process_is_restarted_for_each_generation_case_and_startup_is_separate() {
        let harness = harness();
        let mut sink = MemorySink::default();
        let cancel = AtomicBool::new(false);
        run_with(&harness, &["model-a"], &cancel, &mut sink)
            .0
            .unwrap();
        for record in sink.records.iter().filter(|r| r.model_id == "model-a") {
            assert!(record.timing.process_start_ms.is_some());
            assert!(record.server_settings.is_some());
            assert_eq!(record.server_settings.as_ref().unwrap().cache_prompt, false);
            assert_eq!(record.conditions.page_cache, PageCache::NotControlled);
        }
        for record in sink
            .records
            .iter()
            .filter(|r| r.task == BenchmarkTask::Retrieval)
        {
            assert_eq!(record.model_id, "scripted-embedder");
            assert_eq!(record.timing.process_start_ms, None);
            assert_eq!(record.server_settings, None);
        }
        // The embedding session is unloaded once per retrieval case.
        assert_eq!(harness.embedder.unloads.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn models_run_one_at_a_time_in_the_requested_order() {
        let harness = harness();
        let mut sink = MemorySink::default();
        let cancel = AtomicBool::new(false);
        let (result, order) = run_with(&harness, &["model-b", "model-a"], &cancel, &mut sink);
        result.unwrap();
        assert_eq!(order, vec!["model-b", "model-a"]);
        assert_eq!(harness.live.max.load(Ordering::SeqCst), 1);
        assert_eq!(harness.live.now.load(Ordering::SeqCst), 0);
        let models: Vec<&str> = sink
            .records
            .iter()
            .filter(|r| r.task != BenchmarkTask::Retrieval)
            .map(|r| r.model_id.as_str())
            .collect();
        let first_a = models.iter().position(|m| *m == "model-a").unwrap();
        assert!(models[..first_a].iter().all(|m| *m == "model-b"));
        assert!(models[first_a..].iter().all(|m| *m == "model-a"));
    }

    #[test]
    fn outcomes_come_from_labels_and_a_summary_is_never_graded() {
        let harness = harness();
        let mut sink = MemorySink::default();
        let cancel = AtomicBool::new(false);
        run_with(&harness, &["model-a"], &cancel, &mut sink)
            .0
            .unwrap();
        for record in &sink.records {
            match record.task {
                BenchmarkTask::Summary => {
                    assert_eq!(record.correctness, None);
                    assert!(record.reviews.is_empty());
                }
                BenchmarkTask::Interpretation | BenchmarkTask::Edit => {
                    // The scripted model proposes exactly the labelled edit.
                    assert_eq!(record.correctness, Some(true), "{}", record.case_id);
                }
                BenchmarkTask::Retrieval => assert!(record.correctness.is_some()),
            }
            assert_eq!(record.apply, ApplyOutcome::not_run());
            assert_eq!(record.model_disk_bytes, record.model_file_bytes);
            record.validate().unwrap();
        }
    }

    #[test]
    fn memory_is_attributed_and_unavailable_readings_keep_their_reason() {
        let harness = harness();
        let mut sink = MemorySink::default();
        let cancel = AtomicBool::new(false);
        run_with(&harness, &["model-a"], &cancel, &mut sink)
            .0
            .unwrap();
        for record in &sink.records {
            assert_eq!(record.memory.len(), 1);
            let entry = &record.memory[0];
            if record.task == BenchmarkTask::Retrieval {
                assert_eq!(entry.process, MemoryProcess::Folio);
                assert_eq!(entry.peak_bytes, None);
                assert_eq!(record.peak_process_ram_bytes, None);
                assert!(entry.unavailable_reason.is_some());
            } else {
                assert_eq!(entry.process, MemoryProcess::LlamaServer);
                assert_eq!(entry.pid, Some(4242));
                assert_eq!(record.peak_process_ram_bytes, Some(123_456));
                assert!(entry.scope.contains("process lifetime"));
            }
        }
    }

    #[test]
    fn the_runner_asks_for_prompt_reuse_off_and_counts_requests() {
        let generator = ScriptedGenerator {
            id: "m".into(),
            live: Arc::new(Live::default()),
            restarts: AtomicUsize::new(0),
            requests: AtomicUsize::new(0),
            saw_cache_prompt_off: AtomicBool::new(false),
        };
        generator.live.now.store(1, Ordering::SeqCst);
        let lab = LabProvider::new(&generator);
        let cancel = AtomicBool::new(false);
        let budget = GenerationBudget::default();
        assert_eq!(budget.cache_prompt, None);
        lab.generate_json(&json!({"properties": {}}), &[], &budget, &cancel)
            .unwrap();
        lab.generate_json(&json!({"properties": {}}), &[], &budget, &cancel)
            .unwrap();
        assert!(generator.saw_cache_prompt_off.load(Ordering::SeqCst));
        assert_eq!(lab.take_requests(), 2);
        assert_eq!(lab.take_requests(), 0);
    }

    #[test]
    fn records_say_whether_a_measured_model_is_an_evaluation_candidate() {
        let harness = harness();
        let mut sink = MemorySink::default();
        let cancel = AtomicBool::new(false);
        let (result, _) = run_with(&harness, &["model-a", "candidate-x"], &cancel, &mut sink);
        result.unwrap();
        let generation: Vec<&BenchmarkRecord> = sink
            .records
            .iter()
            .filter(|record| record.task != BenchmarkTask::Retrieval)
            .collect();
        assert!(!generation.is_empty());
        for record in generation {
            let candidate = record.model_id == "candidate-x";
            assert_eq!(
                record.model.evaluation_only, candidate,
                "{}",
                record.model_id
            );
        }
        for record in sink
            .records
            .iter()
            .filter(|r| r.task == BenchmarkTask::Retrieval)
        {
            assert!(
                !record.model.evaluation_only,
                "the embedding model is a product model"
            );
        }
    }

    #[test]
    fn a_model_that_cannot_start_is_recorded_for_each_case_and_the_run_continues() {
        let harness = harness();
        let mut sink = MemorySink::default();
        let cancel = AtomicBool::new(false);
        let (result, order) = run_with(&harness, &["broken-x", "model-a"], &cancel, &mut sink);
        assert_eq!(result.unwrap(), RunEnd::Completed);
        assert_eq!(order, vec!["broken-x", "model-a"]);

        let broken: Vec<&BenchmarkRecord> = sink
            .records
            .iter()
            .filter(|r| r.model_id == "broken-x")
            .collect();
        assert_eq!(broken.len(), 3, "one record per generation case");
        for record in &broken {
            assert_eq!(record.correctness, None);
            assert_eq!(record.timing.requests_in_task, 0);
            assert_eq!(record.objective_checks[0].name, "serverStarted");
            assert_eq!(record.objective_checks[0].passed, Some(false));
            assert!(record.objective_checks[0]
                .detail
                .contains("unsupported architecture"));
            assert_eq!(record.output["stage"], "startup");
            assert!(record.memory[0].peak_bytes.is_none());
            assert!(record.memory[0].unavailable_reason.is_some());
            assert_eq!(record.peak_process_ram_bytes, None);
        }
        // The next model is measured normally, cold then repeat.
        let measured = sink
            .records
            .iter()
            .filter(|r| r.model_id == "model-a")
            .count();
        assert_eq!(measured, 6);
        assert_eq!(harness.live.now.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn generation_records_carry_the_backend_the_server_reported() {
        let base = RuntimeDetail {
            name: RuntimeName::LlamaCpp,
            version: "v".into(),
            backend: Some(crate::lab::record::RuntimeBackend {
                runtime_id: "r".into(),
                platform: "p".into(),
                device_listing: None,
                unavailable_reason: Some("not listed".into()),
                gpu_offload: crate::lab::record::GpuOffload::Disabled,
                observed_log_excerpt: None,
                gpu_layers_offloaded: None,
                layers_total: None,
            }),
        };
        let seen = observed_runtime(&base, Some("load_tensors: offloaded 4/29 layers to GPU"));
        let backend = seen.backend.unwrap();
        assert_eq!(
            backend.gpu_layers_offloaded,
            Some(4),
            "a contradiction is recorded, not hidden"
        );
        assert_eq!(backend.layers_total, Some(29));
        assert_eq!(
            backend.gpu_offload,
            crate::lab::record::GpuOffload::Disabled
        );

        let unchanged = observed_runtime(&base, None);
        assert_eq!(
            unchanged, base,
            "no log means nothing is claimed as observed"
        );
        let onnx = RuntimeDetail {
            backend: None,
            ..base
        };
        assert_eq!(observed_runtime(&onnx, Some("offloaded 1/2")).backend, None);
    }

    #[test]
    fn a_model_that_cannot_start_is_recorded_for_each_case_and_the_run_continues() {
        let harness = harness();
        let mut sink = MemorySink::default();
        let cancel = AtomicBool::new(false);
        let (result, order) = run_with(&harness, &["broken-x", "model-a"], &cancel, &mut sink);
        assert_eq!(result.unwrap(), RunEnd::Completed);
        assert_eq!(order, vec!["broken-x", "model-a"]);

        let broken: Vec<&BenchmarkRecord> = sink
            .records
            .iter()
            .filter(|r| r.model_id == "broken-x")
            .collect();
        assert_eq!(broken.len(), 3, "one record per generation case");
        for record in &broken {
            assert_eq!(record.correctness, None);
            assert_eq!(record.timing.requests_in_task, 0);
            assert_eq!(record.objective_checks[0].name, "serverStarted");
            assert_eq!(record.objective_checks[0].passed, Some(false));
            assert!(record.objective_checks[0]
                .detail
                .contains("unsupported architecture"));
            assert_eq!(record.output["stage"], "startup");
            assert!(record.memory[0].peak_bytes.is_none());
            assert!(record.memory[0].unavailable_reason.is_some());
            assert_eq!(record.peak_process_ram_bytes, None);
        }
        // The next model is measured normally, cold then repeat.
        let measured = sink
            .records
            .iter()
            .filter(|r| r.model_id == "model-a")
            .count();
        assert_eq!(measured, 6);
        assert_eq!(harness.live.now.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn generation_records_carry_the_backend_the_server_reported() {
        let base = RuntimeDetail {
            name: RuntimeName::LlamaCpp,
            version: "v".into(),
            backend: Some(crate::lab::record::RuntimeBackend {
                runtime_id: "r".into(),
                platform: "p".into(),
                device_listing: None,
                unavailable_reason: Some("not listed".into()),
                gpu_offload: crate::lab::record::GpuOffload::Disabled,
                observed_log_excerpt: None,
                gpu_layers_offloaded: None,
                layers_total: None,
            }),
        };
        let seen = observed_runtime(&base, Some("load_tensors: offloaded 4/29 layers to GPU"));
        let backend = seen.backend.unwrap();
        assert_eq!(
            backend.gpu_layers_offloaded,
            Some(4),
            "a contradiction is recorded, not hidden"
        );
        assert_eq!(backend.layers_total, Some(29));
        assert_eq!(
            backend.gpu_offload,
            crate::lab::record::GpuOffload::Disabled
        );

        let unchanged = observed_runtime(&base, None);
        assert_eq!(
            unchanged, base,
            "no log means nothing is claimed as observed"
        );
        let onnx = RuntimeDetail {
            backend: None,
            ..base
        };
        assert_eq!(observed_runtime(&onnx, Some("offloaded 1/2")).backend, None);
    }

    #[test]
    fn every_generation_record_of_a_run_has_the_observed_offload() {
        let harness = harness();
        let mut sink = MemorySink::default();
        let cancel = AtomicBool::new(false);
        run_with(&harness, &["model-a"], &cancel, &mut sink)
            .0
            .unwrap();
        // The scripted factory's runtime has no backend, so nothing is attached;
        // the unit test above covers the attachment itself.
        for record in sink
            .records
            .iter()
            .filter(|r| r.task != BenchmarkTask::Retrieval)
        {
            assert!(record.runtime_detail.backend.is_none());
        }
    }

    #[test]
    fn a_cancelled_run_keeps_what_finished_and_says_so() {
        let harness = harness();
        let mut sink = MemorySink::default();
        let cancel = AtomicBool::new(true);
        let (result, order) = run_with(&harness, &["model-a"], &cancel, &mut sink);
        assert_eq!(result.unwrap(), RunEnd::Cancelled);
        assert!(order.is_empty());
        assert_eq!(sink.runs.last().unwrap().status, RunStatus::Cancelled);
    }

    #[test]
    fn the_disposable_copy_is_gone_after_a_run() {
        let harness = harness();
        let mut sink = MemorySink::default();
        let cancel = AtomicBool::new(false);
        run_with(&harness, &["model-a"], &cancel, &mut sink)
            .0
            .unwrap();
        assert!(!harness
            .dir
            .path()
            .join("model-lab/runs/run-test/workspace")
            .exists());
    }

    struct FailingSink;

    impl LabSink for FailingSink {
        fn record(&mut self, _record: &BenchmarkRecord) -> CoreResult<()> {
            Err(CoreError::Message("disk full".into()))
        }

        fn run_status(&mut self, _run: &RunSummary) -> CoreResult<()> {
            Ok(())
        }
    }

    #[test]
    fn a_sink_that_cannot_save_stops_the_run() {
        let harness = harness();
        let workspaces = LabWorkspaces::new(harness.dir.path());
        let suite = Suite::embedded().unwrap();
        let corpus = Corpus::embedded();
        let factory = ScriptedFactory {
            live: harness.live.clone(),
            order: std::sync::Mutex::new(Vec::new()),
        };
        let probe = FixedProbe;
        let cancel = AtomicBool::new(false);
        let mut sink = FailingSink;
        let result = LabRunner {
            run_id: "run-fail".into(),
            workspaces: &workspaces,
            suite: &suite,
            corpus: &corpus,
            embedding: EmbeddingSubject {
                model: model_ref(
                    "scripted-embedder",
                    crate::contracts::ModelRole::Embedding,
                    500,
                ),
                runtime: RuntimeDetail {
                    name: RuntimeName::OnnxRuntime,
                    version: "scripted-ort".into(),
                    backend: None,
                },
                provider: &harness.embedder,
            },
            generation_model_ids: vec!["model-a".into()],
            factory: &factory,
            probe: &probe,
            sink: &mut sink,
            cancel: &cancel,
            threads: 2,
            host: host_info(),
            clock_ms: fixed_clock,
            progress: None,
        }
        .run();
        assert!(result.is_err());
        assert_eq!(harness.live.opened.load(Ordering::SeqCst), 0);
    }
}

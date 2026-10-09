//! Serde mirror of `BenchmarkRecord` in `src/domain/contracts.ts`.
//!
//! The frozen `BenchmarkResult` fields are flattened into the same JSON object.
//! `fixtures/contracts/benchmark-record.json` pins the encoding for both
//! languages. [`BenchmarkRecord::validate`] states the rules a record must obey
//! before anything stores it.

use crate::contracts::ModelRole;
use crate::error::{CoreError, CoreResult};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

pub const OBSERVATION: &str =
    "single cold/repeat pair; initial observation, not a stable performance estimate";
pub const PROPOSAL_ONLY_REASON: &str = "proposal-only: an actual apply needs explicit native approval; Model Lab never bypasses approval";

/// The payload version written into both JSON columns. A reader refuses any
/// other value instead of guessing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SchemaVersion;

impl Serialize for SchemaVersion {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(1)
    }
}

impl<'de> Deserialize<'de> for SchemaVersion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match u32::deserialize(deserializer)? {
            1 => Ok(SchemaVersion),
            other => Err(D::Error::custom(format!(
                "unsupported Model Lab schemaVersion {other}"
            ))),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BenchmarkTask {
    Retrieval,
    Interpretation,
    Summary,
    Edit,
}

impl BenchmarkTask {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Retrieval => "retrieval",
            Self::Interpretation => "interpretation",
            Self::Summary => "summary",
            Self::Edit => "edit",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PageCache {
    #[serde(rename = "notControlled")]
    NotControlled,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Observation {
    #[serde(
        rename = "single cold/repeat pair; initial observation, not a stable performance estimate"
    )]
    SinglePair,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RequestPosition {
    FirstRequestAfterServerRestart,
    ImmediateRepeat,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum StartupWarmup {
    #[serde(rename = "default-on")]
    DefaultOn,
    #[serde(rename = "disabled")]
    Disabled,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MemoryProcess {
    #[serde(rename = "llama-server")]
    LlamaServer,
    #[serde(rename = "folio")]
    Folio,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RuntimeName {
    #[serde(rename = "llama.cpp")]
    LlamaCpp,
    #[serde(rename = "onnxruntime")]
    OnnxRuntime,
}

/// How a case ended. `valid` means the model produced an answer that the
/// label checks (or, for a summary, a reviewer) can judge. Anything else is a
/// failure to produce one, and it is never the same as "not graded yet".
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OutcomeKind {
    Valid,
    InvalidModelOutput,
    TimedOut,
    RuntimeError,
    Cancelled,
}

impl OutcomeKind {
    /// A retry may change the result unless the case was valid or the user cancelled.
    pub fn retry_needed(self) -> bool {
        !matches!(self, Self::Valid | Self::Cancelled)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ReviewStatus {
    Correct,
    Incorrect,
    PartiallyCorrect,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SuiteRef {
    pub id: String,
    pub sha256: String,
    /// False until a held-out suite is frozen.
    pub frozen: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelFileRef {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}

/// Which catalog a measured model came from. An evaluation candidate is not a
/// supported model: measuring it says nothing about promoting it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ModelCatalog {
    #[serde(rename = "product")]
    Product,
    #[serde(rename = "evaluationCandidate")]
    EvaluationCandidate,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelRef {
    pub id: String,
    pub role: ModelRole,
    pub repo: String,
    pub revision: String,
    pub quantization: String,
    pub files: Vec<ModelFileRef>,
    pub catalog: ModelCatalog,
    /// True exactly for an evaluation candidate.
    pub evaluation_only: bool,
    /// The license the catalog records, as recorded, not a legal conclusion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// A caveat on that license, e.g. conflicting publisher metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license_note: Option<String>,
}

/// What the runtime was asked to do about a GPU. `RuntimeDefault` means Folio
/// passed no offload setting, so the runtime chose; it is never read as CPU-only.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum GpuOffload {
    #[serde(rename = "runtimeDefault")]
    RuntimeDefault,
    /// Folio forced all layers onto the CPU.
    #[serde(rename = "disabled")]
    Disabled,
}

/// The backend facts a runtime reports about itself, kept as observed. The
/// absence of a GPU in a device listing is not evidence that inference ran on
/// the CPU, and a platform without a discrete GPU may still offload.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeBackend {
    pub runtime_id: String,
    /// The manifest's platform label for the runtime build.
    pub platform: String,
    /// `llama-server --list-devices`, as printed; None when it could not be read.
    pub device_listing: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
    /// What Folio asked of the runtime. `Disabled` is a request, and the
    /// observed fields below say what the server reported.
    pub gpu_offload: GpuOffload,
    /// The exact extra launch flags the lab passed (e.g. `--n-gpu-layers 0
    /// --device none`); empty when it passed none.
    #[serde(default)]
    pub flags: Vec<String>,
    /// `Some(true)` only when the server's own output reported zero offloaded
    /// layers and named no GPU backend, after CPU-only was requested.
    /// `Some(false)` when it reported offloading layers. `None` when it cannot
    /// be told (no output, no offload line, or a GPU backend named with no layers
    /// offloaded). Never inferred from the absence of a GPU.
    pub cpu_only_verified: Option<bool>,
    /// Backend and device lines from the server's own startup output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_log_excerpt: Option<String>,
    /// Layers the server reported offloading to a GPU, if it said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_layers_offloaded: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layers_total: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeDetail {
    pub name: RuntimeName,
    pub version: String,
    /// Recorded for llama.cpp rows; the in-process ONNX row has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<RuntimeBackend>,
}

/// `installed_ram_bytes` is installed capacity, never usage.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostInfo {
    pub os: String,
    pub os_version: Option<String>,
    pub arch: String,
    pub cpu_brand: Option<String>,
    pub logical_cpus: u32,
    pub installed_ram_bytes: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Conditions {
    pub n_ctx: u32,
    pub max_output_tokens: u32,
    pub max_passages: u32,
    pub temperature: f32,
    pub seed: i64,
    pub threads: u32,
    pub corpus_sha256: String,
    pub page_cache: PageCache,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Timing {
    pub process_start_ms: Option<u64>,
    pub requests_in_task: u32,
    pub requests_since_process_start: u32,
    pub request_position: RequestPosition,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServerSettings {
    pub startup_warmup: StartupWarmup,
    pub cache_prompt: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryEntry {
    pub process: MemoryProcess,
    pub pid: Option<u32>,
    pub peak_bytes: Option<u64>,
    pub scope: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Check {
    pub name: String,
    /// None when the check could not be evaluated; never a guessed pass.
    pub passed: Option<bool>,
    pub detail: String,
}

/// A person's judgment of one recorded output. Appended, never overwritten.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Review {
    pub status: ReviewStatus,
    pub reviewer: String,
    pub reviewed_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub output_sha256: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ApplyStatus {
    #[serde(rename = "notRun")]
    NotRun,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplyOutcome {
    pub status: ApplyStatus,
    pub reason: String,
}

impl ApplyOutcome {
    pub fn not_run() -> Self {
        Self {
            status: ApplyStatus::NotRun,
            reason: PROPOSAL_ONLY_REASON.to_string(),
        }
    }
}

/// One measured case. The first thirteen fields are the frozen
/// `BenchmarkResult`; the rest is additive.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BenchmarkRecord {
    pub case_id: String,
    pub task: BenchmarkTask,
    pub model_id: String,
    pub revision: String,
    pub quantization: String,
    pub runtime: String,
    pub hardware: String,
    pub context_tokens: u32,
    pub cold: bool,
    pub task_duration_ms: u64,
    pub correctness: Option<bool>,
    pub peak_process_ram_bytes: Option<u64>,
    pub model_disk_bytes: u64,

    pub id: String,
    pub run_id: String,
    pub created_at: u64,
    pub suite: SuiteRef,
    pub prompt_sha256: String,
    pub model: ModelRef,
    pub embedding_model_id: String,
    pub runtime_detail: RuntimeDetail,
    pub host: HostInfo,
    pub conditions: Conditions,
    pub timing: Timing,
    /// None for retrieval rows, which run in-process with no server.
    pub server_settings: Option<ServerSettings>,
    pub observation: Observation,
    pub memory: Vec<MemoryEntry>,
    pub model_file_bytes: u64,
    pub objective_checks: Vec<Check>,
    pub outcome_kind: OutcomeKind,
    /// True exactly when `outcome_kind` is not `valid` or `cancelled`.
    pub retry_needed: bool,
    pub output: Value,
    pub output_sha256: String,
    pub reviews: Vec<Review>,
    pub schema_version: SchemaVersion,
    pub apply: ApplyOutcome,
}

fn invalid(message: impl Into<String>) -> CoreError {
    CoreError::Message(format!("invalid Model Lab record: {}", message.into()))
}

impl BenchmarkRecord {
    /// The rules every record must obey before it is stored or shown.
    pub fn validate(&self) -> CoreResult<()> {
        if self.model_disk_bytes != self.model_file_bytes {
            return Err(invalid("modelDiskBytes must equal modelFileBytes"));
        }
        if self.model.evaluation_only != (self.model.catalog == ModelCatalog::EvaluationCandidate) {
            return Err(invalid(
                "evaluationOnly must be true exactly for an evaluation candidate",
            ));
        }
        if self.context_tokens != self.conditions.n_ctx {
            return Err(invalid("contextTokens must equal conditions.nCtx"));
        }
        let first = self.timing.request_position == RequestPosition::FirstRequestAfterServerRestart;
        if self.cold != first {
            return Err(invalid(
                "cold is true only for the first request after a restart",
            ));
        }
        if self.cold && self.timing.requests_since_process_start != 0 {
            return Err(invalid(
                "a cold record starts at request 0 since process start",
            ));
        }
        if self.retry_needed != self.outcome_kind.retry_needed() {
            return Err(invalid(
                "retryNeeded must be true exactly when the outcome is not valid or cancelled",
            ));
        }
        if !matches!(
            self.outcome_kind,
            OutcomeKind::Valid | OutcomeKind::Cancelled
        ) {
            // A failure to produce an answer is `false` where the labels grade the
            // task and stays `null` for a summary, which is never graded here. The
            // cause is `outcomeKind`, so a failure is never mistaken for "ungraded".
            let expected = if self.task == BenchmarkTask::Summary {
                None
            } else {
                Some(false)
            };
            if self.correctness != expected {
                return Err(invalid(
                    "a failed outcome has correctness false, or null for a summary",
                ));
            }
        }
        if self.task == BenchmarkTask::Summary && self.correctness.is_some() {
            return Err(invalid(
                "summary correctness stays null until a human review",
            ));
        }
        if let Some(backend) = &self.runtime_detail.backend {
            if backend.cpu_only_verified == Some(true) {
                if backend.gpu_offload != GpuOffload::Disabled {
                    return Err(invalid(
                        "cpuOnlyVerified needs CPU-only to have been requested",
                    ));
                }
                if backend
                    .gpu_layers_offloaded
                    .is_some_and(|layers| layers > 0)
                {
                    return Err(invalid(
                        "cpuOnlyVerified cannot be true when layers were offloaded to a GPU",
                    ));
                }
            }
        }
        for entry in &self.memory {
            let reason = entry.unavailable_reason.as_deref().unwrap_or("");
            if entry.peak_bytes.is_none() && reason.is_empty() {
                return Err(invalid("an unavailable peak needs a reason"));
            }
            if entry.peak_bytes.is_some() && !reason.is_empty() {
                return Err(invalid("a measured peak has no unavailable reason"));
            }
            if entry.scope.is_empty() || entry.method.is_empty() {
                return Err(invalid("memory needs a scope and a method"));
            }
        }
        for review in &self.reviews {
            if review.output_sha256 != self.output_sha256 {
                return Err(invalid("a review must name this record's outputSha256"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RunStatus {
    Running,
    Completed,
    Cancelled,
    Failed,
}

/// The `task_type = "run"` row: what was requested and how the run ended.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunSummary {
    pub run_id: String,
    pub status: RunStatus,
    /// The embedding model, then the generation models in the order run.
    pub requested_model_ids: Vec<String>,
    pub suite: SuiteRef,
    pub corpus_sha256: String,
    pub host: HostInfo,
    pub server_settings: ServerSettings,
    pub started_at: u64,
    pub ended_at: Option<u64>,
    /// Building the passage index is not a case; its time is kept here.
    pub index_build_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub schema_version: SchemaVersion,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lab::sink::{LabSink, MemorySink};

    const GOLDEN: &str = include_str!("../../../../../fixtures/contracts/benchmark-record.json");

    fn golden() -> BenchmarkRecord {
        serde_json::from_str(GOLDEN).expect("golden record parses")
    }

    #[test]
    fn the_golden_record_round_trips_without_losing_or_inventing_fields() {
        let record = golden();
        record.validate().expect("golden record is valid");
        let original: Value = serde_json::from_str(GOLDEN).unwrap();
        assert_eq!(serde_json::to_value(&record).unwrap(), original);
    }

    #[test]
    fn an_evaluation_candidate_is_labelled_as_one_in_every_record() {
        let mut record = golden();
        assert_eq!(record.model.catalog, ModelCatalog::Product);
        assert!(!record.model.evaluation_only);

        record.model.catalog = ModelCatalog::EvaluationCandidate;
        assert!(
            record.validate().is_err(),
            "the flag must agree with the catalog"
        );
        record.model.evaluation_only = true;
        record.model.license_note = Some("Unsettled: metadata conflicts.".into());
        record.validate().unwrap();

        let value = serde_json::to_value(&record).unwrap();
        assert_eq!(value["model"]["catalog"], "evaluationCandidate");
        assert_eq!(value["model"]["evaluationOnly"], true);
        assert!(serde_json::from_value::<BenchmarkRecord>(value).is_ok());
    }

    #[test]
    fn a_failed_outcome_is_never_mistaken_for_an_ungraded_one() {
        // A valid summary is ungraded until a person reviews it.
        let summary = golden();
        assert_eq!(summary.outcome_kind, OutcomeKind::Valid);
        assert!(!summary.retry_needed);
        assert_eq!(summary.correctness, None);

        // A summary that timed out is also null, but outcomeKind says why and retry is needed.
        let mut timed_out = golden();
        timed_out.outcome_kind = OutcomeKind::TimedOut;
        assert!(
            timed_out.validate().is_err(),
            "retryNeeded must follow the outcome"
        );
        timed_out.retry_needed = true;
        timed_out.validate().unwrap();

        // An interpretation that timed out is graded false, not null.
        let mut interpretation = golden();
        interpretation.task = BenchmarkTask::Interpretation;
        interpretation.outcome_kind = OutcomeKind::RuntimeError;
        interpretation.retry_needed = true;
        assert!(
            interpretation.validate().is_err(),
            "null would look ungraded"
        );
        interpretation.correctness = Some(false);
        interpretation.validate().unwrap();
        interpretation.correctness = Some(true);
        assert!(
            interpretation.validate().is_err(),
            "a failure cannot be correct"
        );

        // A cancelled case needs no retry.
        let mut cancelled = golden();
        cancelled.outcome_kind = OutcomeKind::Cancelled;
        cancelled.validate().unwrap();

        let value = serde_json::to_value(&timed_out).unwrap();
        assert_eq!(value["outcomeKind"], "timedOut");
        assert_eq!(value["retryNeeded"], true);
    }

    #[test]
    fn an_in_process_retrieval_row_has_no_server_settings_or_start_time() {
        let mut value: Value = serde_json::from_str(GOLDEN).unwrap();
        value["task"] = Value::from("retrieval");
        value["serverSettings"] = Value::Null;
        value["timing"]["processStartMs"] = Value::Null;
        let record: BenchmarkRecord = serde_json::from_value(value.clone()).unwrap();
        record.validate().unwrap();
        assert_eq!(record.server_settings, None);
        assert_eq!(serde_json::to_value(&record).unwrap(), value);
    }

    #[test]
    fn the_backend_is_kept_as_observed_and_never_assumed_to_be_cpu() {
        let record = golden();
        let backend = record
            .runtime_detail
            .backend
            .as_ref()
            .expect("golden has a backend");
        assert_eq!(backend.gpu_offload, GpuOffload::Disabled);
        assert!(backend.device_listing.is_some());
        assert!(backend.observed_log_excerpt.is_some());
        assert_eq!(backend.gpu_layers_offloaded, Some(0));
        assert_eq!(backend.layers_total, Some(29));

        let mut value: Value = serde_json::from_str(GOLDEN).unwrap();
        value["runtimeDetail"]["backend"]["gpuOffload"] = Value::from("cpu");
        assert!(serde_json::from_value::<BenchmarkRecord>(value).is_err());

        let mut value: Value = serde_json::from_str(GOLDEN).unwrap();
        value["runtimeDetail"]
            .as_object_mut()
            .unwrap()
            .remove("backend");
        let without: BenchmarkRecord = serde_json::from_value(value).unwrap();
        assert!(without.runtime_detail.backend.is_none());

        // A disabled request that the server contradicts is kept as data, not rejected.
        let mut contradicted = golden();
        contradicted
            .runtime_detail
            .backend
            .as_mut()
            .unwrap()
            .gpu_layers_offloaded = Some(12);
        contradicted.validate().unwrap();
    }

    #[test]
    fn an_unknown_field_or_schema_version_is_refused() {
        let mut value: Value = serde_json::from_str(GOLDEN).unwrap();
        value["overallScore"] = Value::from(0.9);
        assert!(serde_json::from_value::<BenchmarkRecord>(value).is_err());

        let mut value: Value = serde_json::from_str(GOLDEN).unwrap();
        value["schemaVersion"] = Value::from(2);
        assert!(serde_json::from_value::<BenchmarkRecord>(value).is_err());
    }

    #[test]
    fn a_page_cache_claim_other_than_not_controlled_is_refused() {
        let mut value: Value = serde_json::from_str(GOLDEN).unwrap();
        value["conditions"]["pageCache"] = Value::from("cold");
        assert!(serde_json::from_value::<BenchmarkRecord>(value).is_err());
    }

    #[test]
    fn a_summary_cannot_carry_a_correctness_verdict() {
        let mut record = golden();
        record.correctness = Some(true);
        assert!(record.validate().is_err());
    }

    #[test]
    fn cold_is_only_the_first_request_after_a_restart() {
        let mut record = golden();
        record.cold = false;
        assert!(record.validate().is_err());

        let mut record = golden();
        record.timing.requests_since_process_start = 3;
        assert!(record.validate().is_err());

        let mut repeat = golden();
        repeat.cold = false;
        repeat.timing.request_position = RequestPosition::ImmediateRepeat;
        repeat.timing.requests_since_process_start = 2;
        repeat.validate().expect("an immediate repeat is valid");
    }

    #[test]
    fn memory_needs_a_reason_exactly_when_it_is_unavailable() {
        let mut record = golden();
        record.memory[0].unavailable_reason = None;
        assert!(record.validate().is_err());

        let mut record = golden();
        record.memory[0].peak_bytes = Some(1);
        assert!(record.validate().is_err());
        record.memory[0].unavailable_reason = None;
        record.validate().expect("a measured peak needs no reason");
    }

    #[test]
    fn model_file_bytes_is_the_disk_size_and_a_review_names_its_output() {
        let mut record = golden();
        record.model_disk_bytes += 1;
        assert!(record.validate().is_err());

        let mut record = golden();
        record.reviews.push(Review {
            status: ReviewStatus::Correct,
            reviewer: "tj".into(),
            reviewed_at: 1,
            notes: None,
            output_sha256: "someone-elses-hash".into(),
        });
        assert!(record.validate().is_err());
    }

    #[test]
    fn the_memory_sink_refuses_an_invalid_record() {
        let mut sink = MemorySink::default();
        let mut record = golden();
        sink.record(&record).unwrap();
        record.correctness = Some(false);
        assert!(sink.record(&record).is_err());
        assert_eq!(sink.records.len(), 1);
    }
}

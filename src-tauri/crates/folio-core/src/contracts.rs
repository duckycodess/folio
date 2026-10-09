use serde::{Deserialize, Serialize};

pub type DocumentId = String;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OffsetUnit {
    #[default]
    Utf8Byte,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    En,
    Fil,
    Mixed,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcePassage {
    pub document_id: DocumentId,
    pub document_content_hash: String,
    pub offset_unit: OffsetUnit,
    /// UTF-8 byte offsets into the decoded document text.
    pub start: usize,
    pub end: usize,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentRecord {
    pub id: DocumentId,
    pub workspace_id: String,
    pub relative_path: String,
    pub name: String,
    pub title: String,
    pub language: Language,
    pub media_type: String,
    pub size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_at_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub document: DocumentRecord,
    pub passages: Vec<SourcePassage>,
    pub score: f32,
    pub method: SearchMethod,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_fingerprint: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchMethod {
    Keyword,
    Semantic,
    Hybrid,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingSpace {
    pub model_id: String,
    pub revision: String,
    pub quantization: String,
    pub dimensions: usize,
    pub preprocessing_fingerprint: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroundedSentence {
    pub text: String,
    pub citations: Vec<SourcePassage>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverageRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverageEntry {
    pub document_id: DocumentId,
    pub document_content_hash: String,
    pub offset_unit: OffsetUnit,
    pub ranges: Vec<CoverageRange>,
    pub complete: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroundedAnswer {
    pub text: String,
    pub sources: Vec<SourcePassage>,
    pub coverage: Vec<DocumentId>,
    pub model_id: String,
    pub revision: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroundedResult {
    pub text: String,
    pub sources: Vec<SourcePassage>,
    pub coverage: Vec<DocumentId>,
    pub model_id: String,
    pub revision: String,
    pub kind: GroundedAnswerKind,
    pub sentences: Vec<GroundedSentence>,
    pub coverage_ranges: Vec<CoverageEntry>,
    pub uncited_sentence_count: u32,
    /// What a relationship summary was built from; absent for other results.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basis: Option<SummaryBasis>,
}

/// The connections and files a relationship summary was actually given, as
/// counted by the native core, and whether that is everything.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryBasis {
    pub connections: u32,
    pub files: u32,
    /// True when AI review wasn't finished, or connections or passages were
    /// left out to fit the prompt.
    pub incomplete: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GroundedAnswerKind {
    FileSummary,
    PartialSummary,
    Answer,
    RelationshipSummary,
    ImpactExplanation,
    InsufficientEvidence,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderErrorCode {
    ModelNotInstalled,
    ModelCorrupt,
    RuntimeMissing,
    RuntimeStartFailed,
    GenerationBusy,
    Cancelled,
    ContextLimit,
    InvalidModelOutput,
    EmbeddingSpaceMismatch,
    NoEvidence,
    IoError,
    /// A generation request exceeded its total time budget.
    TimedOut,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeProviderError {
    pub code: ProviderErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelRole {
    Embedding,
    Generation,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelFile {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDescriptor {
    pub id: String,
    pub role: ModelRole,
    pub repo: String,
    pub revision: String,
    pub files: Vec<ModelFile>,
    pub quantization: String,
    pub license: String,
    pub runtime: String,
    pub optional_pack: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelInstallStatus {
    NotInstalled,
    Downloading,
    Verifying,
    Installed,
    Corrupt,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInstallState {
    pub id: String,
    pub status: ModelInstallStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_file_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<NativeProviderError>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum OperationProposal {
    Edit {
        document_id: DocumentId,
        relative_path: String,
        observed_content_hash: String,
        find: String,
        replace: String,
        target_evidence: SourcePassage,
    },
    Rename {
        document_id: DocumentId,
        relative_path: String,
        observed_content_hash: String,
        destination_relative_path: String,
    },
    Move {
        document_id: DocumentId,
        relative_path: String,
        observed_content_hash: String,
        destination_relative_path: String,
    },
    Create {
        destination_relative_path: String,
        content: String,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum InterpretationResult {
    Proposal {
        proposal: OperationProposal,
        request_language: Language,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        exact_duplicate_paths: Vec<String>,
    },
    NeedsFileSelection {
        candidates: Vec<SearchResult>,
        pending_intent: String,
    },
    NeedsClarification {
        question: String,
        reason: String,
    },
    NonMutating {
        intent: NonMutatingIntent,
        #[serde(skip_serializing_if = "Option::is_none")]
        target_query: Option<String>,
    },
    Unsupported {
        reason: String,
    },
    InvalidModelOutput {
        raw_output_digest: String,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NonMutatingIntent {
    Search,
    Summarize,
    Question,
}

#[cfg(test)]
mod contract_tests {
    use super::*;

    const GOLDEN_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../fixtures/contracts");

    fn round_trip(name: &str, value: serde_json::Value) {
        let path = format!("{GOLDEN_ROOT}/{name}.json");
        let expected: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).expect("golden file exists"))
                .expect("golden JSON is valid");
        assert_eq!(value, expected);
    }

    #[test]
    fn native_provider_error_stays_internal() {
        assert_eq!(
            serde_json::to_value(NativeProviderError {
                code: ProviderErrorCode::ModelNotInstalled,
                message: "Install a local model before using AI.".into(),
                detail: Some("generation".into()),
            })
            .unwrap(),
            serde_json::json!({
                "code": "modelNotInstalled",
                "message": "Install a local model before using AI.",
                "detail": "generation"
            })
        );
    }

    #[test]
    fn model_descriptor_uses_native_keys() {
        round_trip(
            "model-descriptor",
            serde_json::to_value(ModelDescriptor {
                id: "qwen3-0.6b-q4".into(),
                role: ModelRole::Generation,
                repo: "unsloth/Qwen3-0.6B-GGUF".into(),
                revision: "revision-a".into(),
                files: vec![ModelFile {
                    path: "Qwen3-0.6B-Q4_K_M.gguf".into(),
                    sha256: "a".repeat(64),
                    bytes: 396_700_000,
                    download_url: None,
                }],
                quantization: "Q4_K_M".into(),
                license: "apache-2.0".into(),
                runtime: "llama.cpp".into(),
                optional_pack: false,
            })
            .unwrap(),
        );
    }

    #[test]
    fn grounded_result_round_trips() {
        let passage = SourcePassage {
            document_id: "fixtures:projects/submission-checklist.md".into(),
            document_content_hash:
                "sha256:8b3538ff1e91ed23104eb5ca6083bf4e44ea37cc8bcb23878d384ca1346bc15a".into(),
            offset_unit: OffsetUnit::Utf8Byte,
            start: 43,
            end: 88,
            text: "Community Learning Project — due October 20".into(),
            page: None,
        };
        round_trip(
            "grounded-answer",
            serde_json::to_value(GroundedResult {
                text: "Community Learning Project is due October 20.".into(),
                sources: vec![passage.clone()],
                coverage: vec!["fixtures:projects/submission-checklist.md".into()],
                model_id: "qwen3-0.6b-q4".into(),
                revision: "revision-a".into(),
                kind: GroundedAnswerKind::FileSummary,
                sentences: vec![GroundedSentence {
                    text: "Community Learning Project is due October 20.".into(),
                    citations: vec![passage],
                }],
                coverage_ranges: vec![CoverageEntry {
                    document_id: "fixtures:projects/submission-checklist.md".into(),
                    document_content_hash:
                        "sha256:8b3538ff1e91ed23104eb5ca6083bf4e44ea37cc8bcb23878d384ca1346bc15a"
                            .into(),
                    offset_unit: OffsetUnit::Utf8Byte,
                    ranges: vec![CoverageRange { start: 0, end: 303 }],
                    complete: true,
                }],
                uncited_sentence_count: 0,
                basis: None,
            })
            .unwrap(),
        );
    }

    #[test]
    fn interpretation_union_round_trips() {
        round_trip(
            "interpretation-result",
            serde_json::to_value(InterpretationResult::NeedsClarification {
                question: "Which file do you mean?".into(),
                reason: "The target description matched multiple files.".into(),
            })
            .unwrap(),
        );
    }
}

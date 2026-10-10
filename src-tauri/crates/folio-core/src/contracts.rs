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
    /// Where the text was generated. Absent on the wire for the local model,
    /// so results stored before online generation still read as local.
    #[serde(default, skip_serializing_if = "GenerationOrigin::is_local")]
    pub origin: GenerationOrigin,
}

/// Where generated text came from: the local model, or the optional online
/// generation the user turned on (ADR 0018).
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GenerationOrigin {
    #[default]
    Local,
    Groq,
}

impl GenerationOrigin {
    pub fn is_local(&self) -> bool {
        *self == Self::Local
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroundedResult {
    pub text: String,
    pub sources: Vec<SourcePassage>,
    pub coverage: Vec<DocumentId>,
    pub model_id: String,
    pub revision: String,
    #[serde(default, skip_serializing_if = "GenerationOrigin::is_local")]
    pub origin: GenerationOrigin,
    pub kind: GroundedAnswerKind,
    pub sentences: Vec<GroundedSentence>,
    pub coverage_ranges: Vec<CoverageEntry>,
    pub uncited_sentence_count: u32,
    /// What a relationship summary was built from; absent for other results.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basis: Option<SummaryBasis>,
    /// Answers about a file the user chose only: nothing in that file passed
    /// the evidence gate or the keyword floor for the question, so the answer
    /// came from its closest or opening passages. Absent, never `false`,
    /// otherwise.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub chosen_file_unmatched: bool,
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
    /// Online generation is on but no key is stored.
    OnlineKeyMissing,
    /// The online service refused the stored key.
    OnlineKeyRejected,
    /// The online service couldn't be reached or failed on its side.
    OnlineUnavailable,
    /// The online service asked Folio to slow down.
    OnlineRateLimited,
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
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
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
#[serde(tag = "status", rename_all = "camelCase", rename_all_fields = "camelCase")]
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
        /// What the chosen file is for, so the chooser can say so.
        #[serde(default)]
        purpose: FileSelectionPurpose,
    },
    NeedsClarification {
        question: String,
        reason: String,
    },
    NonMutating {
        intent: NonMutatingIntent,
        #[serde(skip_serializing_if = "Option::is_none")]
        target_query: Option<String>,
        /// The one file the request names, when it names one. Its `content`
        /// is never set; callers read the file themselves.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        document: Option<DocumentRecord>,
    },
    Unsupported {
        reason: String,
    },
    InvalidModelOutput {
        raw_output_digest: String,
    },
}

/// Why Folio asks which file is meant.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FileSelectionPurpose {
    /// An edit, rename or move.
    #[default]
    Change,
    Summarize,
    Question,
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
    fn proposal_fields_reach_the_frontend_in_camel_case() {
        let result = InterpretationResult::Proposal {
            proposal: OperationProposal::Rename {
                document_id: "workspace:201_NBI Clearance.pdf".into(),
                relative_path: "201_NBI Clearance.pdf".into(),
                observed_content_hash: "sha256:a".into(),
                destination_relative_path: "201_OBI Clearance.pdf".into(),
            },
            request_language: Language::En,
            exact_duplicate_paths: vec!["copy/201_NBI Clearance.pdf".into()],
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "status": "proposal",
                "proposal": {
                    "kind": "rename",
                    "documentId": "workspace:201_NBI Clearance.pdf",
                    "relativePath": "201_NBI Clearance.pdf",
                    "observedContentHash": "sha256:a",
                    "destinationRelativePath": "201_OBI Clearance.pdf"
                },
                "requestLanguage": "en",
                "exactDuplicatePaths": ["copy/201_NBI Clearance.pdf"]
            })
        );
        assert_eq!(serde_json::from_value::<InterpretationResult>(json).unwrap(), result);
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
                origin: GenerationOrigin::Local,
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
                chosen_file_unmatched: false,
            })
            .unwrap(),
        );
    }

    #[test]
    fn only_an_online_result_names_its_origin() {
        let local: GroundedResult = serde_json::from_str(
            &std::fs::read_to_string(format!("{GOLDEN_ROOT}/grounded-answer.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(local.origin, GenerationOrigin::Local);
        assert!(serde_json::to_value(&local).unwrap().get("origin").is_none());

        let online = GroundedResult {
            origin: GenerationOrigin::Groq,
            ..local
        };
        assert_eq!(serde_json::to_value(&online).unwrap()["origin"], "groq");
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

    /// `rename_all` alone renames only the tags; the fields inside each
    /// variant must be camelCase too, or the UI reads them as undefined.
    #[test]
    fn interpretation_fields_use_the_ui_keys() {
        let value = serde_json::to_value(InterpretationResult::Proposal {
            proposal: OperationProposal::Rename {
                document_id: "doc-1".into(),
                relative_path: "Exavault/201_Birth Certificate.pdf".into(),
                observed_content_hash: "hash".into(),
                destination_relative_path: "Exavault/201.pdf".into(),
            },
            request_language: Language::En,
            exact_duplicate_paths: vec!["copy.pdf".into()],
        })
        .unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "status": "proposal",
                "proposal": {
                    "kind": "rename",
                    "documentId": "doc-1",
                    "relativePath": "Exavault/201_Birth Certificate.pdf",
                    "observedContentHash": "hash",
                    "destinationRelativePath": "Exavault/201.pdf",
                },
                "requestLanguage": "en",
                "exactDuplicatePaths": ["copy.pdf"],
            })
        );
        let search = serde_json::to_value(InterpretationResult::NonMutating {
            intent: NonMutatingIntent::Search,
            target_query: Some("birth certificate".into()),
            document: None,
        })
        .unwrap();
        assert_eq!(search["targetQuery"], "birth certificate");
    }
}

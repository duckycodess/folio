//! Serde shapes of the frozen cross-track boundary.
//!
//! Field names serialize in camelCase and match `src/domain/contracts.ts`
//! exactly. Keep the two files in step, and announce a change before merging.

// Several shapes and rules below are the frozen boundary rather than code the
// current commands call: the native writer that consumes them is issue #5.
// They are exercised by this crate's tests and by the shared fixtures, so they
// are kept compiled and checked instead of waiting in a branch.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

use crate::error::FolioError;

/// The only source-offset unit on the boundary: UTF-8 byte offsets into the
/// decoded document text.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum OffsetUnit {
    #[default]
    Utf8Byte,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SourcePassage {
    pub document_id: String,
    /// The revision the offsets refer to, so stale evidence is detectable.
    pub document_content_hash: String,
    pub offset_unit: OffsetUnit,
    pub start: usize,
    pub end: usize,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
}

/// A Folio Ripple review candidate. It is never written to.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImpactCandidate {
    pub document_id: String,
    pub relative_path: String,
    pub reason: String,
    pub evidence: Vec<SourcePassage>,
    pub strength: ImpactStrength,
    /// The relationship that connected this candidate to the target, when one did.
    /// Absent for a byte-identical copy, which is related by content alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relationship_type: Option<RelationshipKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<RelationshipProvenance>,
}

/// The `type` tag of a `Relationship`.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RelationshipKind {
    ExplicitReference,
    Similarity,
    SharedFactCandidate,
}

/// The `provenance` of a `Relationship`.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RelationshipProvenance {
    DocumentLink,
    Embedding,
    Model,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ImpactStrength {
    Evidence,
    SimilarityOnly,
}

/// Explicit destination-absence check: a rename or move is refused when
/// something already occupies the destination.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum DestinationState {
    #[default]
    Absent,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum FileOperation {
    Create {
        destination_relative_path: String,
        media_type: String,
        content: String,
        #[serde(default)]
        expected_destination: DestinationState,
    },
    Edit {
        document_id: String,
        relative_path: String,
        expected_content_hash: String,
        after: String,
    },
    Rename {
        document_id: String,
        relative_path: String,
        expected_content_hash: String,
        destination_relative_path: String,
        #[serde(default)]
        expected_destination: DestinationState,
    },
    Move {
        document_id: String,
        relative_path: String,
        expected_content_hash: String,
        destination_relative_path: String,
        #[serde(default)]
        expected_destination: DestinationState,
    },
    /// Removes one TXT or Markdown file. Its bytes are kept in history first, so
    /// Undo can re-create it while nothing else uses its name.
    Delete {
        document_id: String,
        relative_path: String,
        expected_content_hash: String,
    },
}

/// The `kind` tag of a `FileOperation`, also recorded with each history entry.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FileOperationKind {
    Create,
    Edit,
    Rename,
    Move,
    Delete,
}

impl FileOperationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            FileOperationKind::Create => "create",
            FileOperationKind::Edit => "edit",
            FileOperationKind::Rename => "rename",
            FileOperationKind::Move => "move",
            FileOperationKind::Delete => "delete",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [
            FileOperationKind::Create,
            FileOperationKind::Edit,
            FileOperationKind::Rename,
            FileOperationKind::Move,
            FileOperationKind::Delete,
        ]
        .into_iter()
        .find(|kind| kind.as_str() == value)
    }
}

impl FileOperation {
    pub fn operation_kind(&self) -> FileOperationKind {
        match self {
            FileOperation::Create { .. } => FileOperationKind::Create,
            FileOperation::Edit { .. } => FileOperationKind::Edit,
            FileOperation::Rename { .. } => FileOperationKind::Rename,
            FileOperation::Move { .. } => FileOperationKind::Move,
            FileOperation::Delete { .. } => FileOperationKind::Delete,
        }
    }

    pub fn kind(&self) -> &'static str {
        self.operation_kind().as_str()
    }

    /// The path the operation reads from, if any.
    pub fn source_path(&self) -> Option<&str> {
        match self {
            FileOperation::Create { .. } => None,
            FileOperation::Edit { relative_path, .. }
            | FileOperation::Rename { relative_path, .. }
            | FileOperation::Move { relative_path, .. }
            | FileOperation::Delete { relative_path, .. } => Some(relative_path),
        }
    }

    /// The path the operation would create, if any.
    pub fn destination_path(&self) -> Option<&str> {
        match self {
            FileOperation::Create {
                destination_relative_path,
                ..
            }
            | FileOperation::Rename {
                destination_relative_path,
                ..
            }
            | FileOperation::Move {
                destination_relative_path,
                ..
            } => Some(destination_relative_path),
            FileOperation::Edit { .. } | FileOperation::Delete { .. } => None,
        }
    }

    pub fn expected_content_hash(&self) -> Option<&str> {
        match self {
            FileOperation::Create { .. } => None,
            FileOperation::Edit {
                expected_content_hash,
                ..
            }
            | FileOperation::Rename {
                expected_content_hash,
                ..
            }
            | FileOperation::Move {
                expected_content_hash,
                ..
            }
            | FileOperation::Delete {
                expected_content_hash,
                ..
            } => Some(expected_content_hash),
        }
    }
}

/// Where in Folio a plan was started. The UI names it when it asks for a plan,
/// and it is part of the plan digest, so it can't be relabelled after approval.
/// It is a closed list: no document text can supply it. Plans recorded before
/// sources existed read as `Unknown`.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum PlanSource {
    Home,
    Organize,
    Graph,
    Assistant,
    Summary,
    #[default]
    Unknown,
}

impl PlanSource {
    pub fn as_str(self) -> &'static str {
        match self {
            PlanSource::Home => "home",
            PlanSource::Organize => "organize",
            PlanSource::Graph => "graph",
            PlanSource::Assistant => "assistant",
            PlanSource::Summary => "summary",
            PlanSource::Unknown => "unknown",
        }
    }

    /// The stored value, or `Unknown` for anything else.
    pub fn from_stored(value: &str) -> Self {
        match value {
            "home" => PlanSource::Home,
            "organize" => PlanSource::Organize,
            "graph" => PlanSource::Graph,
            "assistant" => PlanSource::Assistant,
            "summary" => PlanSource::Summary,
            _ => PlanSource::Unknown,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActionPlan {
    pub id: String,
    pub workspace_id: String,
    pub source: PlanSource,
    pub created_at: i64,
    pub expires_at: i64,
    pub operations: Vec<FileOperation>,
    #[serde(default)]
    pub impacts: Vec<ImpactCandidate>,
    pub digest: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    pub plan_id: String,
    pub plan_digest: String,
    pub approved_at: i64,
}

/// Durable per-operation outcome.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum OperationStatus {
    Succeeded,
    Failed,
    Cancelled,
    NotStarted,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OperationOutcome {
    pub operation_index: usize,
    pub status: OperationStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
    /// Present exactly when the status is `succeeded`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_entry_id: Option<String>,
    /// Present exactly when the status is `failed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<FolioError>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BatchStopReason {
    Completed,
    Failed,
    Cancelled,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BatchResult {
    pub plan_id: String,
    pub plan_digest: String,
    pub started_at: i64,
    pub finished_at: i64,
    pub outcomes: Vec<OperationOutcome>,
    pub stop_reason: BatchStopReason,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: String,
    pub plan_id: String,
    pub operation_index: usize,
    pub operation_kind: FileOperationKind,
    pub applied_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_relative_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_relative_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_content_hash: Option<String>,
    /// Absent when the operation removed a path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_content_hash: Option<String>,
    /// False when the previous content could not be retained; Undo is refused.
    pub recoverable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub undone_at: Option<i64>,
}

/// One operation of an applied plan, as Activity shows it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActivityOperation {
    pub operation_index: usize,
    pub operation_kind: FileOperationKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_relative_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_relative_path: Option<String>,
    /// Absent when the batch was recorded before outcomes were stored and this
    /// operation left no history: its outcome is unknown, never guessed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<OperationStatus>,
    /// Present when the operation failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<FolioError>,
    /// Present when the operation changed a file, with its Undo state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history: Option<HistoryEntry>,
}

/// One approved plan that Folio ran: what it did, where it was started, and how
/// it ended. Plans that were prepared or approved but never run are not batches.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActivityBatch {
    pub plan_id: String,
    pub source: PlanSource,
    pub applied_at: i64,
    /// Absent for batches recorded before outcomes were stored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<BatchStopReason>,
    pub operations: Vec<ActivityOperation>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum UndoConflictReason {
    ExternallyModified,
    Missing,
    DestinationOccupied,
    NotRecoverable,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UndoConflict {
    pub history_entry_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_id: Option<String>,
    pub relative_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_content_hash: Option<String>,
    pub observed_content_hash: Option<String>,
    pub reason: UndoConflictReason,
}

/// Undo is whole-batch: if any entry conflicts, no file changes and the
/// blocking file is named.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UndoPreflight {
    pub plan_id: String,
    pub entry_ids: Vec<String>,
    pub conflicts: Vec<UndoConflict>,
    pub undoable: bool,
}

/// A restored preview always needs fresh approval, and mutations never resume
/// automatically after a restart.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RestoredPreview {
    pub plan: ActionPlan,
    pub workspace_available: bool,
    pub requires_fresh_approval: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_operations_with_a_camel_case_kind_tag() {
        let operation = FileOperation::Rename {
            document_id: "w:notes/paalala.md".into(),
            relative_path: "notes/paalala.md".into(),
            expected_content_hash: "sha256:00".into(),
            destination_relative_path: "notes/paalala-oktubre.md".into(),
            expected_destination: DestinationState::Absent,
        };
        let value = serde_json::to_value(&operation).unwrap();
        assert_eq!(value["kind"], serde_json::json!("rename"));
        assert_eq!(
            value["destinationRelativePath"],
            serde_json::json!("notes/paalala-oktubre.md")
        );
        assert_eq!(value["expectedDestination"], serde_json::json!("absent"));
        assert_eq!(value["expectedContentHash"], serde_json::json!("sha256:00"));
    }

    #[test]
    fn a_deletion_names_its_target_and_has_no_destination() {
        let operation = FileOperation::Delete {
            document_id: "w:notes/paalala.md".into(),
            relative_path: "notes/paalala.md".into(),
            expected_content_hash: "sha256:00".into(),
        };
        let value = serde_json::to_value(&operation).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "kind": "delete",
                "documentId": "w:notes/paalala.md",
                "relativePath": "notes/paalala.md",
                "expectedContentHash": "sha256:00",
            })
        );
        assert_eq!(serde_json::from_value::<FileOperation>(value).unwrap(), operation);
        assert_eq!(operation.destination_path(), None);
        assert_eq!(
            FileOperationKind::parse(operation.kind()),
            Some(FileOperationKind::Delete)
        );
    }

    #[test]
    fn serializes_a_passage_with_its_offset_unit_and_revision() {
        let passage = SourcePassage {
            document_id: "w:notes/paalala.md".into(),
            document_content_hash: "sha256:00".into(),
            offset_unit: OffsetUnit::Utf8Byte,
            start: 0,
            end: 4,
            text: "Ang ".into(),
            page: None,
        };
        let value = serde_json::to_value(&passage).unwrap();
        assert_eq!(value["offsetUnit"], serde_json::json!("utf8Byte"));
        assert_eq!(value["documentContentHash"], serde_json::json!("sha256:00"));
        assert!(value.get("page").is_none());
    }

    #[test]
    fn serializes_every_outcome_status_and_stop_reason() {
        let statuses: Vec<_> = [
            OperationStatus::Succeeded,
            OperationStatus::Failed,
            OperationStatus::Cancelled,
            OperationStatus::NotStarted,
        ]
        .iter()
        .map(|status| serde_json::to_value(status).unwrap())
        .collect();
        assert_eq!(
            statuses,
            vec![
                serde_json::json!("succeeded"),
                serde_json::json!("failed"),
                serde_json::json!("cancelled"),
                serde_json::json!("notStarted"),
            ]
        );
        let reasons: Vec<_> = [
            BatchStopReason::Completed,
            BatchStopReason::Failed,
            BatchStopReason::Cancelled,
        ]
        .iter()
        .map(|reason| serde_json::to_value(reason).unwrap())
        .collect();
        assert_eq!(
            reasons,
            vec![
                serde_json::json!("completed"),
                serde_json::json!("failed"),
                serde_json::json!("cancelled"),
            ]
        );
    }
}

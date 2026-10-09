mod active_space;
mod ai_boundary;
mod ai_discovery;
#[cfg(test)]
mod ai_discovery_tests;
mod config_guard;
mod contract_fixtures;
mod contracts;
mod db;
mod embedding_sync;
mod error;
mod evidence;
mod extract;
mod identity;
mod index;
mod lab_commands;
mod lab_store;
mod organize;
mod plan;
#[cfg(test)]
mod relationship_edges_tests;
mod ripple;
mod workspace;
mod writer;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use folio_core::contracts::{
    EmbeddingSpace as ProviderEmbeddingSpace, GroundedResult,
    InterpretationResult, Language, ModelDescriptor, ModelInstallState, ModelInstallStatus,
    ModelRole, NativeProviderError, SearchResult as ProviderSearchResult,
    SourcePassage as CoreSourcePassage,
};
use folio_core::embeddings::{EmbeddingKind, EmbeddingProvider, OrtE5Provider};
use folio_core::error::CoreError;
use folio_core::generation::{GenerationProvider, LlamaServerProvider};
use folio_core::grounding;
use folio_core::interpretation;
use folio_core::models::{DownloadProgress, ModelStore, RuntimeStatus};
use rusqlite::Connection;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

use contracts::{
    ActionPlan, ActivityBatch, Approval, FileOperation, HistoryEntry, ImpactCandidate,
    ImpactStrength, PlanSource, RelationshipKind, UndoPreflight,
};
use error::{error, ErrorCode, FolioError};
use index::{
    AiRelationshipRefresh, ChunkVector, DuplicateGroup, EmbeddingSpace, IndexProgress,
    IndexedDocument, PendingChunk, Relationship, ScanOptions, ScanSummary, SearchResult,
    VectorCandidate,
};
use organize::OrganizationSuggestions;
use plan::PlanRegistry;
use writer::{ApplyReport, RealFileSystem, UndoReport};
use workspace::{
    DocumentListing, DocumentText, KnownWorkspace, ScopedRoot, WorkspaceInfo, WorkspaceRegistry,
};

const INDEX_PROGRESS_EVENT: &str = "folio://index-progress";
/// Reading and embedding progress while an AI request brings the index up to date.
const PREPARING_PROGRESS_EVENT: &str = "folio://preparing-progress";

/// How long a preview stays current. Approval and application both re-check it.
const PLAN_LIFETIME_MS: i64 = 5 * 60 * 1000;

struct Folio {
    workspaces: Mutex<WorkspaceRegistry>,
    plans: Arc<Mutex<PlanRegistry>>,
    /// The persistent index in the OS application-data directory.
    index: Mutex<Connection>,
    index_path: PathBuf,
    /// One scan at a time; a second request waits and then finds little to do.
    scanning: Arc<Mutex<()>>,
    cancel_indexing: Arc<AtomicBool>,
    /// Stops a bounded relationship refresh before its persistence step.
    cancel_relationships: Arc<AtomicBool>,
    /// Stops an apply before its next operation; the running one finishes.
    cancel_apply: Arc<AtomicBool>,
    /// Serializes persistent embedding fills without holding the index or
    /// provider lock across the whole run.
    embedding_sync: Arc<Mutex<()>>,
    cancel_embedding_sync: Arc<AtomicBool>,
    /// Stops the index refresh of the AI request in flight (set by
    /// `cancel_generation`, cleared when the next request starts).
    cancel_ai_request: Arc<AtomicBool>,
    /// One local AI refresh (embedding sync, then relationship discovery) at a time.
    ai_refresh: Arc<Mutex<()>>,
}

impl Folio {
    fn open(index_path: PathBuf) -> Result<Self, FolioError> {
        Ok(Self {
            workspaces: Mutex::new(WorkspaceRegistry::new()),
            plans: Arc::new(Mutex::new(PlanRegistry::new())),
            index: Mutex::new(db::open(&index_path)?),
            index_path,
            scanning: Arc::new(Mutex::new(())),
            cancel_indexing: Arc::new(AtomicBool::new(false)),
            cancel_relationships: Arc::new(AtomicBool::new(false)),
            cancel_apply: Arc::new(AtomicBool::new(false)),
            embedding_sync: Arc::new(Mutex::new(())),
            cancel_embedding_sync: Arc::new(AtomicBool::new(false)),
            cancel_ai_request: Arc::new(AtomicBool::new(false)),
            ai_refresh: Arc::new(Mutex::new(())),
        })
    }

    fn index(&self) -> Result<std::sync::MutexGuard<'_, Connection>, FolioError> {
        self.index.lock().map_err(|_| unavailable_state())
    }

    fn root(&self, workspace_id: &str) -> Result<workspace::ScopedRoot, FolioError> {
        self.workspaces
            .lock()
            .map_err(|_| unavailable_state())?
            .resolve(workspace_id)
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis() as i64)
        .unwrap_or_default()
}

fn core_passage(passage: &contracts::SourcePassage) -> CoreSourcePassage {
    CoreSourcePassage {
        document_id: passage.document_id.clone(),
        document_content_hash: passage.document_content_hash.clone(),
        offset_unit: folio_core::contracts::OffsetUnit::Utf8Byte,
        start: passage.start,
        end: passage.end,
        text: passage.text.clone(),
        page: passage.page,
    }
}

struct SelectedRelationshipSummary {
    focus_rank: u8,
    kind_rank: u8,
    score: f32,
    relationship_type: &'static str,
    provenance: &'static str,
    source_id: String,
    target_id: String,
    passages: Vec<CoreSourcePassage>,
}

fn relationship_passage_is_current(
    passage: &CoreSourcePassage,
    current: &DocumentText,
) -> bool {
    if passage.document_content_hash != current.content_hash
        || passage.offset_unit != folio_core::contracts::OffsetUnit::Utf8Byte
        || passage.start >= passage.end
        || passage.end > current.content.len()
        || !current.content.is_char_boundary(passage.start)
        || !current.content.is_char_boundary(passage.end)
    {
        return false;
    }
    &current.content.as_bytes()[passage.start..passage.end] == passage.text.as_bytes()
}

fn relationship_passage_matches_disk(
    root: &ScopedRoot,
    workspace_id: &str,
    passage: &CoreSourcePassage,
    cache: &mut HashMap<String, Option<DocumentText>>,
) -> bool {
    let current = cache.entry(passage.document_id.clone()).or_insert_with(|| {
        ai_boundary::parse_document_id(workspace_id, &passage.document_id)
            .ok()
            .and_then(|relative| workspace::read_text(&root.path, &relative).ok())
    });
    current
        .as_ref()
        .is_some_and(|current| relationship_passage_is_current(passage, current))
}

fn impact_relationship_label(candidate: &ImpactCandidate) -> &'static str {
    match candidate.relationship_type {
        Some(RelationshipKind::ExplicitReference) => "document link",
        Some(RelationshipKind::Similarity) => "similarity",
        Some(RelationshipKind::SharedFactCandidate) => "shared fact candidate",
        None => "content-only relation",
    }
}

fn impact_strength_label(strength: ImpactStrength) -> &'static str {
    match strength {
        ImpactStrength::Evidence => "evidence",
        ImpactStrength::SimilarityOnly => "similarity-only review hint",
    }
}

/// Runs blocking file work off the async workers.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Result<T, FolioError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|cause| error(ErrorCode::Internal, "A background task stopped unexpectedly.").with_detail("cause", cause.to_string()))
}

/// Remembering a folder for later sessions is best effort: the folder is authorized for
/// this session either way, so a failure to store it must not be reported as a failure
/// to open it.
fn remember_best_effort(state: &Folio, info: &WorkspaceInfo) {
    if let Err(failure) = state.index().and_then(|index| workspace::remember(&index, info)) {
        eprintln!("Folio could not remember {} for later sessions: {}", info.root_path, failure.message);
    }
}

fn unavailable_state() -> FolioError {
    error(
        ErrorCode::Internal,
        "Folio's workspace state is unavailable.",
    )
}

#[tauri::command]
async fn choose_workspace(
    app: AppHandle,
    state: State<'_, Folio>,
) -> Result<Option<WorkspaceInfo>, FolioError> {
    // `blocking_pick_folder` blocks its calling thread until the user
    // answers the dialog. Run it on a dedicated thread (like every other
    // blocking call in this file) so it doesn't tie up an async runtime
    // worker thread — on a second pick, held onto an already-busy worker,
    // that starved every other pending command and made the whole window
    // look frozen until the dialog closed.
    let picked = blocking({
        let app = app.clone();
        move || app.dialog().file().blocking_pick_folder()
    })
    .await?;
    let Some(folder) = picked else {
        return Ok(None);
    };
    let path = folder
        .into_path()
        .map_err(|cause| error(ErrorCode::WorkspaceUnavailable, cause.to_string()))?;
    let info = state.workspaces.lock().map_err(|_| unavailable_state())?.authorize(&path)?;
    remember_best_effort(&state, &info);
    Ok(Some(info))
}

/// Folders chosen in earlier sessions, with whether each is still reachable.
#[tauri::command]
async fn list_workspaces(state: State<'_, Folio>) -> Result<Vec<KnownWorkspace>, FolioError> {
    let remembered = workspace::remembered_workspaces(&*state.index()?)?;
    blocking(move || workspace::with_availability(remembered)).await
}

/// Restores a folder the user picked before. Access is revalidated and the
/// derived identity must still match; the webview never supplies a path.
#[tauri::command]
async fn reopen_workspace(
    state: State<'_, Folio>,
    workspace_id: String,
) -> Result<WorkspaceInfo, FolioError> {
    let path = workspace::remembered_root(&*state.index()?, &workspace_id)?;
    let info = state.workspaces.lock().map_err(|_| unavailable_state())?.authorize(&path)?;
    if info.id != workspace_id {
        return Err(error(
            ErrorCode::WorkspaceUnavailable,
            "That folder now resolves to a different location. Choose it again.",
        ));
    }
    remember_best_effort(&state, &info);
    Ok(info)
}

/// Local Sync: incrementally indexes the folder on its own connection, so
/// search stays responsive. Progress arrives as `folio://index-progress`.
/// `recheck_unreadable` reads every failed or stale document again, even
/// those waiting out a retry backoff.
#[tauri::command]
async fn scan_workspace(
    app: AppHandle,
    state: State<'_, Folio>,
    workspace_id: String,
    recheck_unreadable: Option<bool>,
) -> Result<ScanSummary, FolioError> {
    let root = state.root(&workspace_id)?;
    let index_path = state.index_path.clone();
    let cancel = state.cancel_indexing.clone();
    let scanning = state.scanning.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _scanning = scanning.lock().map_err(|_| unavailable_state())?;
        cancel.store(false, Ordering::SeqCst);
        let mut conn = db::open(&index_path)?;
        let options = ScanOptions {
            recheck_unreadable: recheck_unreadable.unwrap_or(false),
            ..ScanOptions::now()
        };
        index::scan_workspace(&mut conn, &root, &options, &cancel, &mut |progress: &IndexProgress| {
            let _ = app.emit(INDEX_PROGRESS_EVENT, progress);
        })
    })
    .await
    .map_err(|cause| {
        error(ErrorCode::Internal, "Indexing stopped unexpectedly.").with_detail("cause", cause.to_string())
    })?
}

/// "Check again" for specific documents: reads them now, whatever their retry
/// backoff, and returns their updated records. Waits for a running scan.
#[tauri::command]
async fn recheck_documents(
    state: State<'_, Folio>,
    workspace_id: String,
    document_ids: Vec<String>,
) -> Result<Vec<IndexedDocument>, FolioError> {
    let root = state.root(&workspace_id)?;
    let index_path = state.index_path.clone();
    let scanning = state.scanning.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _scanning = scanning.lock().map_err(|_| unavailable_state())?;
        let mut conn = db::open(&index_path)?;
        index::recheck_documents(&mut conn, &root, &document_ids, index::now_ms())
    })
    .await
    .map_err(|cause| {
        error(ErrorCode::Internal, "Checking the documents stopped unexpectedly.").with_detail("cause", cause.to_string())
    })?
}

/// Stops a running scan between files; completed batches are kept.
#[tauri::command]
fn cancel_indexing(state: State<'_, Folio>) {
    state.cancel_indexing.store(true, Ordering::SeqCst);
}

#[tauri::command]
async fn list_indexed_documents(
    state: State<'_, Folio>,
    workspace_id: String,
) -> Result<Vec<IndexedDocument>, FolioError> {
    state.root(&workspace_id)?;
    index::list_documents(&*state.index()?, &workspace_id)
}

/// FTS5 keyword search over the persistent index; results are labelled `keyword`.
#[tauri::command]
async fn search_index(
    state: State<'_, Folio>,
    workspace_id: String,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<SearchResult>, FolioError> {
    state.root(&workspace_id)?;
    index::search(&*state.index()?, &workspace_id, &query, limit.unwrap_or(20))
}

#[tauri::command]
async fn list_duplicates(
    state: State<'_, Folio>,
    workspace_id: String,
) -> Result<Vec<DuplicateGroup>, FolioError> {
    let root = state.root(&workspace_id)?;
    // Candidates come from the index; the byte comparison runs without holding it.
    let candidates = index::duplicate_candidates(&*state.index()?, &workspace_id)?;
    blocking(move || index::verify_duplicates(&root.path, candidates)).await
}

#[tauri::command]
async fn list_relationships(
    app: AppHandle,
    state: State<'_, Folio>,
    workspace_id: String,
    space_fingerprint: Option<String>,
) -> Result<Vec<Relationship>, FolioError> {
    state.root(&workspace_id)?;
    // The model store is read before the index lock; one lock for both reads.
    let selected = selected_embedding_descriptor_lenient(&app);
    let index = state.index()?;
    let active_space =
        active_relationship_space_for(selected.as_ref(), &index, space_fingerprint.as_deref())?;
    index::list_relationships(&index, &workspace_id, active_space.as_deref())
}

/// Runs progressive AI relationship discovery over the vectors #27 persisted
/// for the active space: admission, then fair bounded tiles until nothing is
/// left, the run's comparison budget is spent, a Stop arrives or the selected
/// model changes. Completed tiles always stay. It never starts an embedding
/// producer and holds no lock while comparing.
#[tauri::command]
async fn refresh_ai_connections(
    app: AppHandle,
    state: State<'_, Folio>,
    workspace_id: String,
    space_fingerprint: Option<String>,
) -> Result<AiRelationshipRefresh, FolioError> {
    state.root(&workspace_id)?;
    let index_path = state.index_path.clone();
    let refresh_lock = state.ai_refresh.clone();
    let cancel = state.cancel_relationships.clone();
    Ok(run_blocking::<_, FolioError, _>(move || {
        // One refresh at a time, shared with `refresh_local_ai_index`: taking
        // the lock first means this call can't clear a Stop meant for a running
        // refresh by resetting the shared flag.
        let _refresh = match refresh_lock.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::WouldBlock) => {
                return Err(error(
                    ErrorCode::ProviderBusy,
                    "Folio is already refreshing its local AI index.",
                ))
            }
            Err(std::sync::TryLockError::Poisoned(_)) => return Err(unavailable_state()),
        };
        cancel.store(false, Ordering::SeqCst);
        let mut conn = db::open(&index_path)?;
        let Some(active_space) =
            active_relationship_space(&app, &conn, space_fingerprint.as_deref())?
        else {
            return Ok(AiRelationshipRefresh {
                workspace_id,
                space_fingerprint: None,
                documents_compared: 0,
                relationships_created: 0,
                cancelled: false,
            });
        };
        let still_active = |conn: &Connection| -> Result<bool, FolioError> {
            Ok(active_relationship_space(&app, conn, None)?.as_deref() == Some(active_space.as_str()))
        };
        let summary = ai_discovery::run_discovery(
            &mut conn,
            &ai_discovery::RunContext {
                workspace_id: &workspace_id,
                space: &active_space,
                limits: ai_discovery::DiscoveryLimits::default(),
                cancel: cancel.as_ref(),
                still_active: &still_active,
            },
            &mut |_| {},
        )?;
        let coverage = ai_discovery::coverage(&conn, &workspace_id, Some(&active_space))?;
        Ok(AiRelationshipRefresh {
            workspace_id,
            space_fingerprint: Some(active_space),
            documents_compared: coverage.eligible_documents,
            relationships_created: summary.progress.edges_stored,
            cancelled: summary.end == ai_discovery::RunEnd::Cancelled,
        })
    })
    .await?)
}

#[tauri::command]
fn cancel_ai_connections(state: State<'_, Folio>) {
    state.cancel_relationships.store(true, Ordering::SeqCst);
}

/// Summarizes only the relationship evidence selected by the native index.
/// The model receives passages, never document paths or filesystem capabilities.
#[tauri::command]
async fn summarize_relationships(
    app: AppHandle,
    state: State<'_, Folio>,
    generation_state: State<'_, GenerationState>,
    workspace_id: String,
    document_ids: Vec<String>,
    focus_document_id: Option<String>,
    space_fingerprint: Option<String>,
) -> Result<GroundedResult, FolioError> {
    let root = state.root(&workspace_id)?;
    if document_ids.is_empty() || document_ids.len() > 50 {
        return Err(error(
            ErrorCode::EvidenceInvalid,
            "A relationship summary needs between 1 and 50 documents.",
        ));
    }
    let scope = document_ids
        .iter()
        .map(|document_id| {
            ai_boundary::parse_document_id(&workspace_id, document_id)?;
            Ok::<_, FolioError>(document_id.clone())
        })
        .collect::<Result<HashSet<_>, _>>()?;
    if let Some(focus) = focus_document_id.as_deref() {
        ai_boundary::parse_document_id(&workspace_id, focus)?;
        if !scope.contains(focus) {
            return Err(error(
                ErrorCode::EvidenceInvalid,
                "The relationship-summary focus must be in the requested document scope.",
            ));
        }
    }
    let index_path = state.index_path.clone();
    let scanning = state.scanning.clone();
    let generation_state = generation_state.inner().clone();
    Ok(run_blocking::<_, FolioError, _>(move || {
        let active_space = {
            let conn = db::open(&index_path)?;
            active_relationship_space(&app, &conn, space_fingerprint.as_deref())?
        };
        let relationships = {
            let _scanning = scanning.lock().map_err(|_| unavailable_state())?;
            let conn = db::open(&index_path)?;
            index::list_relationships(
                &conn,
                &workspace_id,
                active_space.as_deref(),
            )?
        };
        let mut selected = Vec::<SelectedRelationshipSummary>::new();
        for relationship in relationships {
            let (
                source_id,
                target_id,
                kind_rank,
                score,
                relationship_type,
                provenance,
                passages,
            ) = match relationship {
                Relationship::ExplicitReference(reference) => (
                    reference.source_id,
                    reference.target_id,
                    0,
                    0.0,
                    reference.relationship_type,
                    reference.provenance,
                    reference.evidence.iter().map(core_passage).collect(),
                ),
                Relationship::Similarity(similarity) => (
                    similarity.source_id,
                    similarity.target_id,
                    2,
                    similarity.score,
                    similarity.relationship_type,
                    similarity.provenance,
                    similarity
                        .source_evidence
                        .iter()
                        .chain(similarity.target_evidence.iter())
                        .map(core_passage)
                        .collect(),
                ),
                Relationship::SharedFactCandidate(shared) => (
                    shared.source_id,
                    shared.target_id,
                    1,
                    shared.confidence.unwrap_or(0.0),
                    shared.relationship_type,
                    shared.provenance,
                    shared
                        .source_evidence
                        .iter()
                        .chain(shared.target_evidence.iter())
                        .map(core_passage)
                        .collect(),
                ),
            };
            if !scope.contains(&source_id) || !scope.contains(&target_id) {
                continue;
            }
            let focus_rank = focus_document_id.as_deref().map_or(1, |focus| {
                if source_id == focus || target_id == focus { 0 } else { 1 }
            });
            selected.push(SelectedRelationshipSummary {
                focus_rank,
                kind_rank,
                score,
                relationship_type,
                provenance,
                source_id,
                target_id,
                passages,
            });
        }
        selected.sort_by(|left, right| {
            left.focus_rank
                .cmp(&right.focus_rank)
                .then(left.kind_rank.cmp(&right.kind_rank))
                .then_with(|| right.score.total_cmp(&left.score))
                .then_with(|| left.source_id.cmp(&right.source_id))
                .then_with(|| left.target_id.cmp(&right.target_id))
        });
        let coverage = {
            let conn = db::open(&index_path)?;
            ai_discovery::coverage(&conn, &workspace_id, active_space.as_deref())?
        };
        let mut current_documents = HashMap::<String, Option<DocumentText>>::new();
        for relationship in &mut selected {
            relationship.passages.retain(|passage| {
                relationship_passage_matches_disk(
                    &root,
                    &workspace_id,
                    passage,
                    &mut current_documents,
                )
            });
        }
        selected.retain(|relationship| !relationship.passages.is_empty());
        let available_connections = selected.len();
        let mut seen = HashSet::new();
        let mut passages = Vec::new();
        for relationship in &selected {
            for passage in &relationship.passages {
                let key = (
                    passage.document_id.clone(),
                    passage.start,
                    passage.end,
                );
                if seen.insert(key) {
                    passages.push(passage.clone());
                    if passages.len() >= folio_core::generation::MAX_PASSAGES {
                        break;
                    }
                }
            }
            if passages.len() >= folio_core::generation::MAX_PASSAGES {
                break;
            }
        }
        let passage_keys = passages
            .iter()
            .map(|passage| {
                (
                    passage.document_id.clone(),
                    passage.start,
                    passage.end,
                )
            })
            .collect::<HashSet<_>>();
        let entries = selected
            .into_iter()
            .filter_map(|relationship| {
                let passages = relationship
                    .passages
                    .into_iter()
                    .filter(|passage| {
                        passage_keys.contains(&(
                            passage.document_id.clone(),
                            passage.start,
                            passage.end,
                        ))
                    })
                    .collect::<Vec<_>>();
                (!passages.is_empty()).then(|| {
                    folio_core::grounding::RelationshipSummaryEntry {
                        relationship_type: relationship.relationship_type.into(),
                        provenance: relationship.provenance.into(),
                        source_id: relationship.source_id,
                        target_id: relationship.target_id,
                        passages,
                    }
                })
            })
            .collect::<Vec<_>>();
        let language_text = passages
            .iter()
            .map(|passage| passage.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let language = grounding::detect_language(&language_text);
        if passages.is_empty() {
            return Ok(grounding::answer_question(
                None,
                grounding::RELATIONSHIP_SUMMARY_INSTRUCTION,
                passages,
                language,
                &AtomicBool::new(false),
            )?);
        }
        // What the model is actually given, counted here and not by the UI:
        // incomplete when AI review wasn't finished or connections were left
        // out to fit the prompt's passage cap.
        let files = entries
            .iter()
            .flat_map(|entry| [entry.source_id.as_str(), entry.target_id.as_str()])
            .collect::<HashSet<_>>()
            .len();
        let basis = folio_core::contracts::SummaryBasis {
            connections: entries.len() as u32,
            files: files as u32,
            incomplete: coverage.state != ai_discovery::CoverageState::Complete
                || coverage.overflow_documents > 0
                || entries.len() < available_connections,
        };
        let lease = acquire_generation(&app, &generation_state)?;
        let result = grounding::relationship_summary(
            lease.provider.as_ref(),
            entries,
            language,
            lease.claim.cancel.as_ref(),
        );
        // Release the slot before the result is shaped (the lease's drop
        // would release it anyway, on every path).
        drop(lease);
        let mut result = result?;
        if result.kind == folio_core::contracts::GroundedAnswerKind::RelationshipSummary {
            result.basis = Some(basis);
        }
        Ok(result)
    })
    .await?)
}

/// Explains one native Ripple candidate without changing its plan or any file.
#[tauri::command]
async fn explain_impact(
    app: AppHandle,
    state: State<'_, Folio>,
    generation_state: State<'_, GenerationState>,
    workspace_id: String,
    plan_id: String,
    document_id: String,
) -> Result<GroundedResult, FolioError> {
    let root = state.root(&workspace_id)?;
    let candidate = {
        let plans = state.plans.lock().map_err(|_| unavailable_state())?;
        let plan = plan_in_workspace(&plans, &plan_id, &workspace_id)?;
        if now_ms() >= plan.expires_at {
            return Err(error(
                ErrorCode::PlanExpired,
                "This preview is no longer current. Review a fresh preview.",
            )
            .with_detail("planId", plan_id));
        }
        plan.impacts
            .iter()
            .find(|impact| impact.document_id == document_id)
            .cloned()
            .ok_or_else(|| {
                error(
                    ErrorCode::EvidenceInvalid,
                    "That Ripple candidate is not part of this preview.",
                )
                .with_detail("documentId", document_id.clone())
            })?
    };
    let relative_path = ai_boundary::parse_document_id(&workspace_id, &document_id)?;
    let generation_state = generation_state.inner().clone();
    Ok(run_blocking::<_, FolioError, _>(move || {
        for passage in &candidate.evidence {
            if passage.document_id != document_id {
                return Err(error(
                    ErrorCode::EvidenceInvalid,
                    "Ripple evidence names a different document.",
                )
                .with_detail("reason", "wrongEvidenceDocument"));
            }
        }
        if candidate.evidence.is_empty() {
            return Ok(grounding::answer_question(
                None,
                grounding::IMPACT_EXPLANATION_INSTRUCTION,
                Vec::new(),
                Language::Unknown,
                &AtomicBool::new(false),
            )?);
        }
        let current = workspace::read_text(&root.path, &relative_path)?;
        for passage in &candidate.evidence {
            if passage.document_content_hash != current.content_hash
                || passage.start >= passage.end
                || passage.end > current.content.len()
                || !current.content.is_char_boundary(passage.start)
                || !current.content.is_char_boundary(passage.end)
                || current.content.as_bytes().get(passage.start..passage.end)
                    != Some(passage.text.as_bytes())
            {
                return Err(error(
                    ErrorCode::EvidenceInvalid,
                    "This Ripple evidence is stale. Review the file again before asking for an explanation.",
                )
                .with_detail("reason", "staleEvidence"));
            }
        }
        let language = grounding::detect_language(
            &candidate
                .evidence
                .iter()
                .map(|passage| passage.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let lease = acquire_generation(&app, &generation_state)?;
        let result = grounding::impact_explanation(
            lease.provider.as_ref(),
            impact_relationship_label(&candidate),
            impact_strength_label(candidate.strength),
            &candidate.reason,
            candidate.evidence.iter().map(core_passage).collect(),
            language,
            lease.claim.cancel.as_ref(),
        );
        Ok(result?)
    })
    .await?)
}

/// Returns the space fingerprint; vectors are only compared within one space.
#[tauri::command]
async fn register_embedding_space(
    state: State<'_, Folio>,
    space: EmbeddingSpace,
) -> Result<String, FolioError> {
    index::register_space(&*state.index()?, &space)
}

#[tauri::command]
async fn pending_embedding_chunks(
    state: State<'_, Folio>,
    workspace_id: String,
    space_fingerprint: String,
    limit: Option<usize>,
) -> Result<Vec<PendingChunk>, FolioError> {
    state.root(&workspace_id)?;
    index::pending_embedding_chunks(&*state.index()?, &workspace_id, &space_fingerprint, limit.unwrap_or(64))
}

#[tauri::command]
async fn put_embeddings(
    state: State<'_, Folio>,
    workspace_id: String,
    space_fingerprint: String,
    items: Vec<ChunkVector>,
) -> Result<usize, FolioError> {
    state.root(&workspace_id)?;
    index::put_embeddings(&mut *state.index()?, &workspace_id, &space_fingerprint, &items)
}

#[tauri::command]
async fn vector_candidates(
    state: State<'_, Folio>,
    workspace_id: String,
    space_fingerprint: String,
    vector: Vec<f32>,
    k: Option<usize>,
) -> Result<Vec<VectorCandidate>, FolioError> {
    state.root(&workspace_id)?;
    index::vector_candidates(&*state.index()?, &workspace_id, &space_fingerprint, &vector, k.unwrap_or(20))
}

fn refuse_during_lab(lab_state: &lab_commands::LabState) -> Result<(), FolioError> {
    if lab_state
        .lock()
        .map_err(|_| unavailable_state())?
        .is_some()
    {
        return Err(error(
            ErrorCode::ProviderBusy,
            "Model Lab is measuring models. Try again when it finishes.",
        )
        .with_detail("reason", "modelLabRunning"));
    }
    Ok(())
}

struct NativePassageEmbedder {
    app: AppHandle,
    embedding_state: EmbeddingState,
    lab_state: lab_commands::LabState,
}

impl embedding_sync::PassageEmbedder for NativePassageEmbedder {
    fn embed_batch(
        &mut self,
        texts: &[String],
        cancel: &AtomicBool,
    ) -> db::NativeResult<(ProviderEmbeddingSpace, Vec<Vec<f32>>)> {
        refuse_during_lab(&self.lab_state)?;
        let embedded = with_embedding_provider_guarded(
            &self.app,
            &self.embedding_state,
            || refuse_during_lab(&self.lab_state),
            |provider| {
                let vectors = provider
                    .embed(texts, EmbeddingKind::Passage, Some(cancel))
                    .map_err(native_error)?;
                Ok((provider.space().clone(), vectors))
            },
        )?;
        embedded.ok_or_else(|| {
            error(
                ErrorCode::ModelNotInstalled,
                "Select a verified local embedding model first.",
            )
            .with_detail("component", "embedding")
        })
    }
}

impl evidence::Embedder for NativePassageEmbedder {
    fn provider_space(&mut self) -> db::NativeResult<Option<ProviderEmbeddingSpace>> {
        refuse_during_lab(&self.lab_state)?;
        with_embedding_provider_guarded(
            &self.app,
            &self.embedding_state,
            || refuse_during_lab(&self.lab_state),
            |provider| Ok(provider.space().clone()),
        )
    }

    fn embed_query(
        &mut self,
        text: &str,
        cancel: &AtomicBool,
    ) -> db::NativeResult<folio_core::embeddings::QueryEmbedding> {
        with_embedding_provider_guarded(
            &self.app,
            &self.embedding_state,
            || refuse_during_lab(&self.lab_state),
            |provider| provider.embed_query(text, Some(cancel)).map_err(native_error),
        )?
        .ok_or_else(|| {
            error(
                ErrorCode::ModelNotInstalled,
                "The selected embedding model is no longer installed.",
            )
            .with_detail("component", "embedding")
        })
    }
}

/// Everything one AI request needs to read the persistent index. Cheap to
/// build on the async side, then moved into the blocking task.
struct AiRequest {
    app: AppHandle,
    root: ScopedRoot,
    index_path: PathBuf,
    scanning: Arc<Mutex<()>>,
    embedding_sync: Arc<Mutex<()>>,
    cancel: Arc<AtomicBool>,
    embedding_state: EmbeddingState,
    lab_state: lab_commands::LabState,
}

impl AiRequest {
    fn new(
        app: &AppHandle,
        state: &Folio,
        embedding_state: &EmbeddingState,
        lab_state: &lab_commands::LabState,
        workspace_id: &str,
    ) -> Result<Self, FolioError> {
        Ok(Self {
            app: app.clone(),
            root: ai_boundary::resolve_workspace(state, workspace_id)?,
            index_path: state.index_path.clone(),
            scanning: state.scanning.clone(),
            embedding_sync: state.embedding_sync.clone(),
            cancel: state.cancel_ai_request.clone(),
            embedding_state: embedding_state.clone(),
            lab_state: lab_state.clone(),
        })
    }

    /// Runs `work` against the folder's persistent index on its own
    /// connection. The first thing `work` asks of the index brings it up to
    /// date, reporting `folio://preparing-progress` meanwhile.
    fn run<R>(
        self,
        work: impl FnOnce(&mut evidence::LocalIndex<'_, NativePassageEmbedder>) -> Result<R, FolioError>,
    ) -> Result<R, FolioError> {
        self.cancel.store(false, Ordering::Release);
        let mut conn = db::open(&self.index_path)?;
        let mut embedder = NativePassageEmbedder {
            app: self.app.clone(),
            embedding_state: self.embedding_state.clone(),
            lab_state: self.lab_state.clone(),
        };
        let app = self.app.clone();
        let mut sink = move |progress: evidence::PreparingProgress| {
            let _ = app.emit(PREPARING_PROGRESS_EVENT, progress);
        };
        let mut index = evidence::LocalIndex::new(
            &mut conn,
            &self.root,
            &self.scanning,
            &self.embedding_sync,
            &mut embedder,
            &self.cancel,
            &mut sink,
        );
        work(&mut index)
    }
}

#[tauri::command]
async fn sync_embeddings(
    app: AppHandle,
    state: State<'_, Folio>,
    embedding_state: State<'_, EmbeddingState>,
    lab_state: State<'_, lab_commands::LabState>,
    workspace_id: String,
) -> Result<embedding_sync::EmbeddingSyncSummary, FolioError> {
    refuse_during_lab(lab_state.inner())?;
    state.root(&workspace_id)?;
    let index_path = state.index_path.clone();
    let sync_lock = state.embedding_sync.clone();
    let cancel = state.cancel_embedding_sync.clone();
    let embedding_state = embedding_state.inner().clone();
    let lab_state = lab_state.inner().clone();
    Ok(run_blocking(move || {
        run_embedding_sync(
            app,
            index_path,
            &sync_lock,
            &cancel,
            embedding_state,
            lab_state,
            workspace_id,
        )
    })
    .await?)
}

/// #27's persistent fill: load the selected provider's space, register the
/// stored-chunk space it produces, then embed pending chunks. Shared by the
/// `sync_embeddings` command and the combined local AI refresh.
fn run_embedding_sync(
    app: AppHandle,
    index_path: PathBuf,
    sync_lock: &Mutex<()>,
    cancel: &AtomicBool,
    embedding_state: EmbeddingState,
    lab_state: lab_commands::LabState,
    workspace_id: String,
) -> Result<embedding_sync::EmbeddingSyncSummary, FolioError> {
    refuse_during_lab(&lab_state)?;
    let _sync_guard = match sync_lock.try_lock() {
        Ok(guard) => guard,
        Err(std::sync::TryLockError::WouldBlock) => {
            return Err(error(
                ErrorCode::ProviderBusy,
                "Another embedding sync is already running.",
            ))
        }
        Err(std::sync::TryLockError::Poisoned(_)) => return Err(unavailable_state()),
    };
    cancel.store(false, Ordering::Release);

    let provider_space = with_embedding_provider_guarded(
        &app,
        &embedding_state,
        || refuse_during_lab(&lab_state),
        |provider| Ok(provider.space().clone()),
    )?
    .ok_or_else(|| {
        error(
            ErrorCode::ModelNotInstalled,
            "Select a verified local embedding model first.",
        )
        .with_detail("component", "embedding")
    })?;
    let stored_space = embedding_sync::stored_index_space(&provider_space)?;

    let mut conn = db::open(&index_path)?;
    let space_fingerprint = index::register_space(&conn, &stored_space)?;
    let mut store = embedding_sync::IndexChunkStore::new(
        &mut conn,
        workspace_id.clone(),
        space_fingerprint.clone(),
    );
    let mut embedder = NativePassageEmbedder {
        app,
        embedding_state,
        lab_state,
    };
    embedding_sync::sync_embeddings(
        &mut store,
        &mut embedder,
        &provider_space,
        &space_fingerprint,
        workspace_id,
        cancel,
        embedding_sync::SyncLimits::default(),
    )
}

#[tauri::command]
fn cancel_embedding_sync(state: State<'_, Folio>) {
    state.cancel_embedding_sync.store(true, Ordering::Release);
}

const AI_REFRESH_PROGRESS_EVENT: &str = "folio://ai-refresh-progress";
/// Discovery runs one refresh may take before returning; the coverage in the
/// result says honestly whether anything is left.
const MAX_DISCOVERY_RUNS_PER_REFRESH: usize = 4;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AiRefreshProgress {
    workspace_id: String,
    /// `embedding`, `admitting` or `relationships`.
    phase: &'static str,
    tiles: usize,
    pairs_completed: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LocalAiRefresh {
    workspace_id: String,
    /// The embedding phase's summary; absent when it didn't run.
    #[serde(skip_serializing_if = "Option::is_none")]
    embedding: Option<embedding_sync::EmbeddingSyncSummary>,
    /// The discovery run's progress; absent when it didn't run.
    #[serde(skip_serializing_if = "Option::is_none")]
    discovery: Option<ai_discovery::DiscoveryProgress>,
    /// Why discovery stopped, when it did: `complete`, `budgetExhausted`,
    /// `cancelled` or `spaceChanged`. Absent when no search model is ready.
    #[serde(skip_serializing_if = "Option::is_none")]
    ended: Option<ai_discovery::RunEnd>,
    coverage: ai_discovery::RelationshipCoverage,
}

/// Refreshes Folio's local AI index for a folder: #27's embedding sync, then
/// progressive relationship discovery in the resulting active space. One
/// refresh at a time and one Stop (`cancel_local_ai_refresh`) for both phases.
/// Completed work always stays, so an interrupted refresh resumes.
#[tauri::command]
async fn refresh_local_ai_index(
    app: AppHandle,
    state: State<'_, Folio>,
    embedding_state: State<'_, EmbeddingState>,
    lab_state: State<'_, lab_commands::LabState>,
    workspace_id: String,
) -> Result<LocalAiRefresh, FolioError> {
    refuse_during_lab(lab_state.inner())?;
    state.root(&workspace_id)?;
    let index_path = state.index_path.clone();
    let refresh_lock = state.ai_refresh.clone();
    let sync_lock = state.embedding_sync.clone();
    let cancel_sync = state.cancel_embedding_sync.clone();
    let cancel = state.cancel_relationships.clone();
    let embedding_state = embedding_state.inner().clone();
    let lab_state = lab_state.inner().clone();
    Ok(run_blocking::<_, FolioError, _>(move || {
        let _refresh = match refresh_lock.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::WouldBlock) => {
                return Err(error(
                    ErrorCode::ProviderBusy,
                    "Folio is already refreshing its local AI index.",
                ))
            }
            Err(std::sync::TryLockError::Poisoned(_)) => return Err(unavailable_state()),
        };
        cancel.store(false, Ordering::SeqCst);
        cancel_sync.store(false, Ordering::SeqCst);
        let emit = |phase: &'static str, tiles: usize, pairs_completed: usize| {
            let _ = app.emit(
                AI_REFRESH_PROGRESS_EVENT,
                AiRefreshProgress { workspace_id: workspace_id.clone(), phase, tiles, pairs_completed },
            );
        };

        emit("embedding", 0, 0);
        let embedding = match run_embedding_sync(
            app.clone(),
            index_path.clone(),
            &sync_lock,
            &cancel_sync,
            embedding_state,
            lab_state,
            workspace_id.clone(),
        ) {
            Ok(summary) => Some(summary),
            // No selected, installed search model: browsing and links keep
            // working, and coverage says there is no active space.
            Err(failure) if failure.code == ErrorCode::ModelNotInstalled => None,
            Err(failure) => return Err(failure),
        };
        let mut conn = db::open(&index_path)?;
        let stopped_early = embedding.as_ref().is_some_and(|summary| summary.cancelled);
        let Some(active_space) = active_relationship_space(&app, &conn, None)? else {
            let coverage = ai_discovery::coverage(&conn, &workspace_id, None)?;
            return Ok(LocalAiRefresh { workspace_id, embedding, discovery: None, ended: None, coverage });
        };
        if stopped_early {
            let coverage = ai_discovery::coverage(&conn, &workspace_id, Some(&active_space))?;
            return Ok(LocalAiRefresh {
                workspace_id,
                embedding,
                discovery: None,
                ended: Some(ai_discovery::RunEnd::Cancelled),
                coverage,
            });
        }

        emit("admitting", 0, 0);
        ai_discovery::purge_other_spaces(&mut conn, &workspace_id, &active_space)?;
        let still_active = |conn: &Connection| -> Result<bool, FolioError> {
            Ok(active_relationship_space(&app, conn, None)?.as_deref() == Some(active_space.as_str()))
        };
        let mut total = ai_discovery::DiscoveryProgress::default();
        let mut ended = ai_discovery::RunEnd::Complete;
        for _ in 0..MAX_DISCOVERY_RUNS_PER_REFRESH {
            let before = total.clone();
            let run = ai_discovery::run_discovery(
                &mut conn,
                &ai_discovery::RunContext {
                    workspace_id: &workspace_id,
                    space: &active_space,
                    limits: ai_discovery::DiscoveryLimits::default(),
                    cancel: cancel.as_ref(),
                    still_active: &still_active,
                },
                &mut |progress| emit("relationships", before.tiles + progress.tiles, before.pairs_completed + progress.pairs_completed),
            )?;
            total.admitted += run.progress.admitted;
            total.tiles += run.progress.tiles;
            total.comparisons += run.progress.comparisons;
            total.work += run.progress.work;
            total.pairs_completed += run.progress.pairs_completed;
            total.edges_stored += run.progress.edges_stored;
            ended = run.end;
            if ended != ai_discovery::RunEnd::BudgetExhausted {
                break;
            }
        }
        let coverage = ai_discovery::coverage(&conn, &workspace_id, Some(&active_space))?;
        Ok(LocalAiRefresh {
            workspace_id,
            embedding,
            discovery: Some(total),
            ended: Some(ended),
            coverage,
        })
    })
    .await?)
}

/// One Stop for both phases of `refresh_local_ai_index`.
#[tauri::command]
fn cancel_local_ai_refresh(state: State<'_, Folio>) {
    state.cancel_embedding_sync.store(true, Ordering::Release);
    state.cancel_relationships.store(true, Ordering::SeqCst);
}

/// What Folio has compared for AI connections in the active search model's
/// index. Reads only; never starts work.
#[tauri::command]
async fn relationship_coverage(
    app: AppHandle,
    state: State<'_, Folio>,
    workspace_id: String,
) -> Result<ai_discovery::RelationshipCoverage, FolioError> {
    state.root(&workspace_id)?;
    let selected = selected_embedding_descriptor_lenient(&app);
    let index = state.index()?;
    let active = active_space::resolve_installed_descriptor(&index, selected.as_ref())?;
    ai_discovery::coverage(&index, &workspace_id, active.as_deref())
}

#[tauri::command]
async fn list_documents(
    state: State<'_, Folio>,
    workspace_id: String,
) -> Result<DocumentListing, FolioError> {
    let workspaces = state.workspaces.lock().map_err(|_| unavailable_state())?;
    let root = workspaces.resolve(&workspace_id)?;
    workspace::list_documents(&root)
}

#[tauri::command]
async fn read_document(
    state: State<'_, Folio>,
    workspace_id: String,
    relative_path: String,
) -> Result<DocumentText, FolioError> {
    // Resolve, release the registry, then read: a PDF can take a while to extract.
    let root = state.root(&workspace_id)?;
    blocking(move || workspace::read_text(&root.path, &relative_path)).await?
}

/// Prepare an exact plan. Nothing is written: the plan is checked against the
/// current files and stored so that an approval can be bound to it.
#[tauri::command]
async fn prepare_plan(
    app: AppHandle,
    state: State<'_, Folio>,
    workspace_id: String,
    source: PlanSource,
    operations: Vec<FileOperation>,
    impacts: Option<Vec<ImpactCandidate>>,
) -> Result<ActionPlan, FolioError> {
    // `unknown` only describes plans recorded before sources existed.
    if source == PlanSource::Unknown {
        return Err(error(ErrorCode::OperationUnsupported, "Say where in Folio this change was started.").with_detail("source", source.as_str()));
    }
    // Read the model store before taking any lock, and only when Folio computes
    // Ripple itself; only AI rows of the active space reach it. A store that
    // can't be read gives links-only Ripple, never a refused preview.
    let selected = if impacts.is_none() { selected_embedding_descriptor_lenient(&app) } else { None };
    let workspaces = state.workspaces.lock().map_err(|_| unavailable_state())?;
    let root = workspaces.resolve(&workspace_id)?;
    // Ripple evidence comes from the index and each edit's diff unless the caller
    // supplies it (for example with the exact phrase an interpreter replaced).
    let impacts = match impacts {
        Some(impacts) => impacts,
        None => {
            let index = state.index()?;
            let active_space = active_space::resolve_installed_descriptor(&index, selected.as_ref())?;
            ripple::plan_impacts(&index, &root, &operations, active_space.as_deref())?
        }
    };
    let mut plans = state.plans.lock().map_err(|_| unavailable_state())?;
    let now = now_ms();
    let plan = plans.prepare(&workspace_id, source, operations, impacts, now, PLAN_LIFETIME_MS)?;
    plan::preflight_plan(&root.path, &plan, now)?;
    Ok(plan)
}

/// A plan identity is only honoured in the workspace it was prepared for.
fn plan_in_workspace(plans: &PlanRegistry, plan_id: &str, workspace_id: &str) -> Result<ActionPlan, FolioError> {
    let plan = plans.plan(plan_id)?.clone();
    if plan.workspace_id != workspace_id {
        return Err(error(ErrorCode::PlanUnknown, "That preview belongs to a different folder.").with_detail("planId", plan_id));
    }
    Ok(plan)
}

/// Approve a plan Folio prepared. The caller echoes the digest it was shown, so
/// an approval can never apply to different operations.
#[tauri::command]
async fn approve_plan(
    state: State<'_, Folio>,
    workspace_id: String,
    plan_id: String,
    plan_digest: String,
) -> Result<Approval, FolioError> {
    let workspaces = state.workspaces.lock().map_err(|_| unavailable_state())?;
    workspaces.resolve(&workspace_id)?;
    let mut plans = state.plans.lock().map_err(|_| unavailable_state())?;
    plan_in_workspace(&plans, &plan_id, &workspace_id)?;
    plans.approve(&plan_id, &plan_digest, now_ms())
}

/// Applies an approved plan through the native writer. The approval, digest, expiry
/// and every target are checked again first; each operation's outcome is durable, and
/// the plan is retired so its approval cannot be used twice. It runs off the async
/// workers on its own index connection: waiting for a running scan, writing files and
/// re-indexing them can take a while.
#[tauri::command]
async fn apply_plan(
    state: State<'_, Folio>,
    workspace_id: String,
    plan_id: String,
) -> Result<ApplyReport, FolioError> {
    let root = state.root(&workspace_id)?;
    let (plans, scanning, cancel, index_path) = (state.plans.clone(), state.scanning.clone(), state.cancel_apply.clone(), state.index_path.clone());
    blocking(move || -> Result<ApplyReport, FolioError> {
        // A scan must not read files halfway through a batch. Waiting for it comes first,
        // so the plan registry is not held meanwhile and expiry is judged after the wait.
        let _scanning = scanning.lock().map_err(|_| unavailable_state())?;
        let (plan, approval, now) = {
            let plans = plans.lock().map_err(|_| unavailable_state())?;
            let plan = plan_in_workspace(&plans, &plan_id, &workspace_id)?;
            let now = now_ms();
            plans.assert_can_apply(&root.path, &plan_id, now)?;
            let approval = plans.approval(&plan_id).cloned().ok_or_else(|| {
                error(ErrorCode::ApprovalRequired, "Approve this exact plan before any file changes.").with_detail("planId", plan_id.as_str())
            })?;
            (plan, approval, now)
        };
        // Applies are serialized by the scan lock, and an applied plan is refused by its
        // durable record, so the registry need not stay locked while files are written.
        cancel.store(false, Ordering::SeqCst);
        let report = writer::apply_plan(&mut db::open(&index_path)?, &root, &plan, &approval, now, &RealFileSystem, &cancel)?;
        plans.lock().map_err(|_| unavailable_state())?.finish(&plan_id);
        Ok(report)
    })
    .await?
}

/// Stops a running apply before its next operation. Finished changes are kept.
#[tauri::command]
fn cancel_apply(state: State<'_, Folio>) {
    state.cancel_apply.store(true, Ordering::SeqCst);
}

/// The Undo preview: what would be reversed and anything blocking it. Writes nothing.
#[tauri::command]
async fn preview_undo(
    state: State<'_, Folio>,
    workspace_id: String,
    plan_id: String,
) -> Result<UndoPreflight, FolioError> {
    let root = state.root(&workspace_id)?;
    writer::preview_undo(&*state.index()?, &root, &plan_id)
}

/// Reverses an applied plan. `entry_ids` must be exactly those of the preview the user
/// confirmed; if anything changed since, nothing is undone.
#[tauri::command]
async fn undo_plan(
    state: State<'_, Folio>,
    workspace_id: String,
    plan_id: String,
    entry_ids: Vec<String>,
) -> Result<UndoReport, FolioError> {
    let root = state.root(&workspace_id)?;
    let (scanning, index_path) = (state.scanning.clone(), state.index_path.clone());
    blocking(move || {
        let _scanning = scanning.lock().map_err(|_| unavailable_state())?;
        writer::undo_plan(&mut db::open(&index_path)?, &root, &plan_id, &entry_ids, now_ms(), &RealFileSystem)
    })
    .await?
}

#[tauri::command]
async fn list_history(
    state: State<'_, Folio>,
    workspace_id: String,
    limit: Option<usize>,
) -> Result<Vec<HistoryEntry>, FolioError> {
    state.root(&workspace_id)?;
    writer::list_history(&*state.index()?, &workspace_id, limit.unwrap_or(100))
}

/// Activity: the plans Folio ran, newest first, one entry per batch with every
/// operation's outcome. `before` is the plan id the previous page ended with.
#[tauri::command]
async fn list_activity(
    state: State<'_, Folio>,
    workspace_id: String,
    limit: Option<usize>,
    before: Option<String>,
) -> Result<Vec<ActivityBatch>, FolioError> {
    state.root(&workspace_id)?;
    writer::list_activity(&*state.index()?, &workspace_id, limit.unwrap_or(50), before.as_deref())
}

/// Ripple for an explicit phrase, e.g. the value an interpreter knows it replaced.
#[tauri::command]
async fn ripple_impacts(
    app: AppHandle,
    state: State<'_, Folio>,
    workspace_id: String,
    document_id: String,
    replaced_text: String,
) -> Result<Vec<ImpactCandidate>, FolioError> {
    state.root(&workspace_id)?;
    let selected = selected_embedding_descriptor_lenient(&app);
    let index = state.index()?;
    let active_space = active_space::resolve_installed_descriptor(&index, selected.as_ref())?;
    let document = index::get_document(&index, &workspace_id, &document_id)?;
    ripple::impacts(&index, &workspace_id, &document, &replaced_text, active_space.as_deref())
}

/// Builds the edit operation that replaces one exact passage of a document.
#[tauri::command]
async fn prepare_passage_edit(
    state: State<'_, Folio>,
    workspace_id: String,
    document_id: String,
    before: String,
    after: String,
) -> Result<FileOperation, FolioError> {
    let root = state.root(&workspace_id)?;
    writer::passage_edit(&*state.index()?, &root, &document_id, &before, &after)
}

#[tauri::command]
async fn organization_suggestions(
    state: State<'_, Folio>,
    workspace_id: String,
) -> Result<OrganizationSuggestions, FolioError> {
    let root = state.root(&workspace_id)?;
    let (filenames, candidates) = {
        let index = state.index()?;
        (organize::filename_suggestions(&index, &root)?, index::duplicate_candidates(&index, &workspace_id)?)
    };
    // Duplicate candidates are confirmed byte for byte without holding the index.
    blocking(move || OrganizationSuggestions { duplicate_groups: index::verify_duplicates(&root.path, candidates), filenames }).await
}


/* ------------------------------------------------ issue #4 local AI providers */

struct EmbeddingSlot {
    model_id: String,
    revision: String,
    provider: OrtE5Provider,
}

type EmbeddingState = Arc<Mutex<Option<EmbeddingSlot>>>;

struct GenerationSlot {
    model_id: String,
    revision: String,
    provider: Arc<LlamaServerProvider>,
}

#[derive(Default)]
struct GenerationStateInner {
    slot: Option<GenerationSlot>,
    active_cancel: Option<Arc<AtomicBool>>,
    /// Set while the llama.cpp runtime is reinstalled, so no request starts a
    /// server from the directory being replaced.
    runtime_installing: bool,
    /// How many unloads are waiting for the slot's holder. While any is, no
    /// request or lab run may claim the slot, so an unload never cancels work
    /// that started after it was asked for.
    unloading: usize,
}

/// Marks an unload in progress for as long as it lives, on every exit path.
struct UnloadingMark(GenerationState);

impl UnloadingMark {
    fn new(guard: &mut GenerationStateInner, generation_state: &GenerationState) -> Self {
        guard.unloading += 1;
        Self(generation_state.clone())
    }
}

impl Drop for UnloadingMark {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.0.lock() {
            guard.unloading = guard.unloading.saturating_sub(1);
        }
    }
}

type GenerationState = Arc<Mutex<GenerationStateInner>>;
type InstallState = Arc<Mutex<Option<Arc<AtomicBool>>>>;


#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderInstallState {
    id: String,
    status: ModelInstallStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_file_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<FolioError>,
}

fn provider_install_state(state: ModelInstallState) -> ProviderInstallState {
    ProviderInstallState {
        id: state.id,
        status: state.status,
        model_file_bytes: state.model_file_bytes,
        error: state.error.map(ai_boundary::provider_failure),
    }
}

fn native_error(error: CoreError) -> NativeProviderError {
    match error {
        CoreError::Provider(provider) => provider.into_native(),
        other => NativeProviderError {
            code: folio_core::contracts::ProviderErrorCode::IoError,
            message: other.to_string(),
            detail: None,
        },
    }
}

fn app_data_dir(app: &AppHandle) -> Result<PathBuf, NativeProviderError> {
    app.path()
        .app_data_dir()
        .map_err(|error| NativeProviderError {
            code: folio_core::contracts::ProviderErrorCode::IoError,
            message: "Folio could not locate its application-data directory.".into(),
            detail: Some(error.to_string()),
        })
}

fn model_store(app: &AppHandle) -> Result<ModelStore, NativeProviderError> {
    ModelStore::new(app_data_dir(app)?).map_err(native_error)
}

/// Selects the one persistent relationship space Folio is allowed to expose:
/// #27's stored-chunk space for the selected, installed embedding model,
/// derived from its descriptor (`active_space`) and required to be registered.
/// The model store is read here, before any index or scan lock, and the
/// embedding mutex is never taken. An optional webview fingerprint is an
/// assertion, never a selector.
fn active_relationship_space(
    app: &AppHandle,
    conn: &Connection,
    requested_space: Option<&str>,
) -> Result<Option<String>, FolioError> {
    let selected = selected_embedding_descriptor(app)?;
    active_relationship_space_for(selected.as_ref(), conn, requested_space)
}

/// `active_relationship_space` for a descriptor already read, so callers can
/// read the model store before taking the index lock.
fn active_relationship_space_for(
    selected: Option<&ModelDescriptor>,
    conn: &Connection,
    requested_space: Option<&str>,
) -> Result<Option<String>, FolioError> {
    let active = active_space::resolve_installed_descriptor(conn, selected)?;
    if let (Some(active), Some(requested)) = (active.as_deref(), requested_space) {
        if active != requested {
            return Err(error(
                ErrorCode::EmbeddingSpaceMismatch,
                "The requested relationship space is not the selected installed model's active space.",
            )
            .with_detail("requestedSpaceFingerprint", requested)
            .with_detail("activeSpaceFingerprint", active));
        }
    }
    Ok(active)
}

/// Like `selected_embedding_descriptor`, for paths that only read AI rows to
/// display them (file previews, Ripple, Connections, coverage). A model store
/// that can't be read means no active space, so links-only results: it never
/// stops a user from previewing or reviewing a change.
fn selected_embedding_descriptor_lenient(app: &AppHandle) -> Option<ModelDescriptor> {
    lenient_descriptor(selected_embedding_descriptor(app))
}

fn lenient_descriptor(read: Result<Option<ModelDescriptor>, FolioError>) -> Option<ModelDescriptor> {
    read.unwrap_or_else(|failure| {
        eprintln!("Folio is showing links only: the model store could not be read: {}", failure.message);
        None
    })
}

/// The selected embedding model's descriptor when it is installed. Reads the
/// model store only: no index lock, no embedding mutex, no ONNX load.
fn selected_embedding_descriptor(app: &AppHandle) -> Result<Option<ModelDescriptor>, FolioError> {
    let store = model_store(app)?;
    let Some(model_id) = store.selected_model(ModelRole::Embedding)? else {
        return Ok(None);
    };
    let descriptor = store.model(&model_id)?.clone();
    let installed = matches!(store.model_state(&model_id)?.status, ModelInstallStatus::Installed);
    Ok(installed.then_some(descriptor))
}

async fn run_blocking<T, E, F>(work: F) -> Result<T, E>
where
    T: Send + 'static,
    E: From<NativeProviderError> + Send + 'static,
    F: FnOnce() -> Result<T, E> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| {
            NativeProviderError {
                code: folio_core::contracts::ProviderErrorCode::IoError,
                message: "The native operation stopped unexpectedly.".into(),
                detail: Some(error.to_string()),
            }
            .into()
        })?
}

fn begin_install(state: &InstallState) -> Result<Arc<AtomicBool>, NativeProviderError> {
    let mut guard = state.lock().map_err(|_| NativeProviderError {
        code: folio_core::contracts::ProviderErrorCode::IoError,
        message: "The model installation state is unavailable.".into(),
        detail: None,
    })?;
    if guard.is_some() {
        return Err(NativeProviderError {
            code: folio_core::contracts::ProviderErrorCode::GenerationBusy,
            message: "Another model or runtime installation is active.".into(),
            detail: None,
        });
    }
    let cancel = Arc::new(AtomicBool::new(false));
    *guard = Some(cancel.clone());
    Ok(cancel)
}

fn finish_install(
    state: &InstallState,
    cancel: &Arc<AtomicBool>,
) -> Result<(), NativeProviderError> {
    let mut guard = state.lock().map_err(|_| NativeProviderError {
        code: folio_core::contracts::ProviderErrorCode::IoError,
        message: "The model installation state is unavailable.".into(),
        detail: None,
    })?;
    if guard
        .as_ref()
        .is_some_and(|active| Arc::ptr_eq(active, cancel))
    {
        *guard = None;
    }
    Ok(())
}

fn unload_embedding(embedding_state: &EmbeddingState) -> Result<(), NativeProviderError> {
    let mut guard = embedding_state.lock().map_err(|_| NativeProviderError {
        code: folio_core::contracts::ProviderErrorCode::IoError,
        message: "The local embedding state is unavailable.".into(),
        detail: None,
    })?;
    if let Some(slot) = guard.take() {
        slot.provider.unload().map_err(native_error)?;
    }
    Ok(())
}

#[tauri::command]
async fn list_models(app: AppHandle) -> Result<Vec<ModelDescriptor>, FolioError> {
    Ok(
        run_blocking::<_, FolioError, _>(move || Ok(model_store(&app)?.manifest().models.clone()))
            .await?,
    )
}

/// The install state every view reads on load. It uses the per-session hash
/// cache (a full SHA-256 once, then size and modified time), like a launch
/// does: the store's uncached `verify_model` re-hashed every GGUF each time a
/// view mounted or a model was selected, which made selection crawl and left
/// other models "Checking…" with no way to choose them.
#[tauri::command]
async fn verify_model(
    app: AppHandle,
    model_id: String,
) -> Result<ProviderInstallState, FolioError> {
    let state = run_blocking(move || {
        model_store(&app)?
            .model_state(&model_id)
            .map_err(native_error)
    })
    .await?;
    Ok(provider_install_state(state))
}

#[tauri::command]
async fn install_model(
    app: AppHandle,
    embedding_state: State<'_, EmbeddingState>,
    install_state: State<'_, InstallState>,
    model_id: String,
) -> Result<ProviderInstallState, FolioError> {
    let cancel = begin_install(install_state.inner())?;
    let worker_cancel = cancel.clone();
    let install_state = install_state.inner().clone();
    let embedding_state = embedding_state.inner().clone();
    let progress_app = app.clone();
    let result = run_blocking(move || {
        let result = model_store(&app)?
            .install_model(&model_id, &worker_cancel, |progress| {
                let _ = progress_app.emit("folio://model-progress", progress);
            })
            .map_err(native_error);
        unload_embedding(&embedding_state)?;
        result
    })
    .await;
    finish_install(&install_state, &cancel)?;
    Ok(provider_install_state(result?))
}

#[tauri::command]
async fn remove_model(
    app: AppHandle,
    embedding_state: State<'_, EmbeddingState>,
    generation_state: State<'_, GenerationState>,
    install_state: State<'_, InstallState>,
    model_id: String,
) -> Result<(), FolioError> {
    let embedding_state = embedding_state.inner().clone();
    let generation_state = generation_state.inner().clone();
    let install_state = install_state.inner().clone();
    let result = run_blocking(move || {
        // Serialized with installs and selections, and nothing keeps the
        // model's files open while they are deleted.
        let lock = begin_install(&install_state)?;
        let result = (|| {
            unload_generation_for_model(&generation_state, &model_id)?;
            unload_embedding(&embedding_state)?;
            model_store(&app)?
                .remove_model(&model_id)
                .map_err(native_error)
        })();
        finish_install(&install_state, &lock)?;
        result
    })
    .await;
    result?;
    Ok(())
}

/// Stop the generation server if it is serving `model_id`.
fn unload_generation_for_model(
    generation_state: &GenerationState,
    model_id: &str,
) -> Result<(), NativeProviderError> {
    let serving = generation_state
        .lock()
        .map_err(|_| NativeProviderError {
            code: folio_core::contracts::ProviderErrorCode::IoError,
            message: "The local generation state is unavailable.".into(),
            detail: None,
        })?
        .slot
        .as_ref()
        .is_some_and(|slot| slot.model_id == model_id);
    if serving {
        unload_generation_now(generation_state)?;
    }
    Ok(())
}

#[tauri::command]
async fn select_model(
    app: AppHandle,
    embedding_state: State<'_, EmbeddingState>,
    install_state: State<'_, InstallState>,
    role: ModelRole,
    model_id: String,
) -> Result<(), FolioError> {
    let embedding_selection = matches!(&role, ModelRole::Embedding);
    let embedding_state = embedding_state.inner().clone();
    let install_state = install_state.inner().clone();
    let result = run_blocking(move || {
        let lock = begin_install(&install_state)?;
        let result = model_store(&app)?
            .select_model(role, &model_id)
            .map_err(native_error);
        let unloaded = if embedding_selection {
            unload_embedding(&embedding_state)
        } else {
            Ok(())
        };
        finish_install(&install_state, &lock)?;
        unloaded?;
        result
    })
    .await;
    result?;
    Ok(())
}

#[tauri::command]
async fn runtime_status(app: AppHandle, runtime_id: String) -> Result<RuntimeStatus, FolioError> {
    Ok(run_blocking::<_, FolioError, _>(move || {
        Ok(model_store(&app)?
            .runtime_status(&runtime_id)
            .map_err(native_error)?)
    })
    .await?)
}

#[tauri::command]
async fn install_runtime(
    app: AppHandle,
    runtime_id: String,
    install_state: State<'_, InstallState>,
    generation_state: State<'_, GenerationState>,
) -> Result<RuntimeStatus, FolioError> {
    let cancel = begin_install(install_state.inner())?;
    let worker_cancel = cancel.clone();
    let install_state = install_state.inner().clone();
    let generation_state = generation_state.inner().clone();
    // A running llama-server keeps its directory in use (on Windows the swap
    // would fail), so stop it first and keep new requests out until the new
    // runtime is in place. A request already running is not cut off.
    if let Err(failure) = begin_runtime_install(&generation_state) {
        finish_install(&install_state, &cancel)?;
        return Err(failure.into());
    }
    let progress_app = app.clone();
    let worker_generation_state = generation_state.clone();
    let result = run_blocking(move || {
        unload_generation_now(&worker_generation_state)?;
        model_store(&app)?
            .install_runtime(&runtime_id, &worker_cancel, |progress: DownloadProgress| {
                let _ = progress_app.emit("folio://runtime-progress", progress);
            })
            .map_err(native_error)
    })
    .await;
    end_runtime_install(&generation_state);
    finish_install(&install_state, &cancel)?;
    Ok(result?)
}

/// Refuses a runtime reinstall while a generation request is running, and
/// otherwise marks the runtime as being installed so no request starts one.
fn begin_runtime_install(generation_state: &GenerationState) -> Result<(), NativeProviderError> {
    let mut guard = generation_state.lock().map_err(|_| NativeProviderError {
        code: folio_core::contracts::ProviderErrorCode::IoError,
        message: "The local generation state is unavailable.".into(),
        detail: None,
    })?;
    if guard.active_cancel.is_some() {
        return Err(NativeProviderError {
            code: folio_core::contracts::ProviderErrorCode::GenerationBusy,
            message: "Stop the running request before reinstalling the local AI runtime.".into(),
            detail: None,
        });
    }
    guard.runtime_installing = true;
    Ok(())
}

fn end_runtime_install(generation_state: &GenerationState) {
    if let Ok(mut guard) = generation_state.lock() {
        guard.runtime_installing = false;
    }
}

#[tauri::command]
fn cancel_install(install_state: State<'_, InstallState>) -> Result<(), FolioError> {
    let guard = install_state.lock().map_err(|_| NativeProviderError {
        code: folio_core::contracts::ProviderErrorCode::IoError,
        message: "The model installation state is unavailable.".into(),
        detail: None,
    })?;
    if let Some(cancel) = guard.as_ref() {
        cancel.store(true, Ordering::Release);
    }
    Ok(())
}

/// What the model setup screen needs and can't learn from the manifest
/// listing: the saved selections, and the runtime build for this computer.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelSetup {
    selected_embedding: Option<String>,
    selected_generation: Option<String>,
    host_runtime_id: &'static str,
    /// Exact download size of that runtime from the pinned manifest.
    host_runtime_bytes: Option<u64>,
    /// The whole device's physical RAM, never Folio's own process memory.
    device_memory_bytes: Option<u64>,
    /// Free space on the disk that holds Folio's models.
    available_disk_bytes: Option<u64>,
}

#[tauri::command]
async fn model_setup(app: AppHandle) -> Result<ModelSetup, FolioError> {
    Ok(run_blocking::<_, FolioError, _>(move || {
        let store = model_store(&app)?;
        let host_runtime_id = runtime_id_for_host();
        Ok(ModelSetup {
            selected_embedding: store
                .selected_model(ModelRole::Embedding)
                .map_err(native_error)?,
            selected_generation: store
                .selected_model(ModelRole::Generation)
                .map_err(native_error)?,
            host_runtime_id,
            host_runtime_bytes: store
                .manifest()
                .runtimes
                .iter()
                .find(|runtime| runtime.id == host_runtime_id)
                .map(|runtime| runtime.files.iter().map(|file| file.bytes).sum()),
            device_memory_bytes: folio_core::device::total_memory_bytes(),
            available_disk_bytes: folio_core::device::available_disk_bytes(store.data_dir()),
        })
    })
    .await?)
}

fn runtime_id_for_host() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "llama-b11524-win-cpu-x64"
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "llama-b11524-macos-arm64"
    }
    #[cfg(all(target_os = "macos", not(target_arch = "aarch64")))]
    {
        "llama-b11524-macos-x64"
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        "llama-b11524-ubuntu-x64"
    }
}

fn read_ai_document(root: &ScopedRoot, relative_path: &str) -> Result<DocumentText, FolioError> {
    workspace::read_text(&root.path, relative_path)
}

/// Run the guarded part of an embedding operation while holding the provider
/// slot lock. The guard runs before any operation can load or replace the
/// slot. The lock order is `EmbeddingState -> LabState`; Model Lab releases
/// its `LabState` guard before it unloads the embedding slot.
fn with_embedding_state_guarded<T, G, F>(
    embedding_state: &EmbeddingState,
    before_load: G,
    work: F,
) -> Result<T, FolioError>
where
    G: FnOnce() -> Result<(), FolioError>,
    F: FnOnce(&mut Option<EmbeddingSlot>) -> Result<T, FolioError>,
{
    let mut guard = embedding_state.lock().map_err(|_| unavailable_state())?;
    before_load()?;
    work(&mut *guard)
}

fn with_embedding_provider_guarded<T, G, F>(
    app: &AppHandle,
    embedding_state: &EmbeddingState,
    before_load: G,
    work: F,
) -> Result<Option<T>, FolioError>
where
    G: FnOnce() -> Result<(), FolioError>,
    F: FnOnce(&OrtE5Provider) -> Result<T, NativeProviderError>,
{
    let store = model_store(app)?;
    with_embedding_state_guarded(embedding_state, before_load, |guard| {
        let Some(model_id) = store
            .selected_model(ModelRole::Embedding)
            .map_err(native_error)?
        else {
            if let Some(slot) = guard.take() {
                slot.provider.unload().map_err(native_error)?;
            }
            return Ok(None);
        };
        let descriptor = store.model(&model_id).map_err(native_error)?.clone();
        let state = store.model_state(&model_id).map_err(native_error)?;
        if !matches!(
            state.status,
            folio_core::contracts::ModelInstallStatus::Installed
        ) {
            if let Some(slot) = guard.take() {
                slot.provider.unload().map_err(native_error)?;
            }
            return Ok(None);
        }
        if guard.as_ref().is_none_or(|slot| {
            slot.model_id != descriptor.id || slot.revision != descriptor.revision
        }) {
            if let Some(slot) = guard.take() {
                slot.provider.unload().map_err(native_error)?;
            }
            let e5 = folio_core::embeddings::e5_inputs_from_descriptor(&descriptor)
                .map_err(native_error)?;
            let model_path = store
                .verified_file_path(&model_id, &e5.model_file)
                .map_err(native_error)?;
            let tokenizer_path = store
                .verified_file_path(&model_id, &e5.tokenizer_file)
                .map_err(native_error)?;
            let provider = OrtE5Provider::from_files(
                model_path,
                tokenizer_path,
                e5.inputs.model_id.clone(),
                e5.inputs.revision.clone(),
                e5.inputs.quantization.clone(),
                e5.inputs.dimensions,
                &e5.inputs.model_sha256,
                &e5.inputs.tokenizer_sha256,
                e5.inputs.max_tokens,
                folio_core::embeddings::DEFAULT_BATCH_SIZE,
                2,
            )
            .map_err(native_error)?;
            *guard = Some(EmbeddingSlot {
                model_id: descriptor.id.clone(),
                revision: descriptor.revision.clone(),
                provider,
            });
        }
        let slot = guard.as_ref().ok_or_else(|| {
            NativeProviderError {
                code: folio_core::contracts::ProviderErrorCode::IoError,
                message: "The local embedding provider is unavailable.".into(),
                detail: None,
            }
        })?;
        Ok(Some(work(&slot.provider)?))
    })
}

/// What the persistent index holds for a folder, from SQLite alone: nothing is
/// read from the files and no model is loaded. `embeddedChunkCount` is known
/// once a request has loaded the embedding model.
#[tauri::command]
async fn index_status(
    state: State<'_, Folio>,
    embedding_state: State<'_, EmbeddingState>,
    workspace_id: Option<String>,
) -> Result<evidence::IndexStatus, FolioError> {
    let Some(workspace_id) = workspace_id else {
        return Ok(evidence::IndexStatus::empty());
    };
    state.root(&workspace_id)?;
    let loaded_space = embedding_state
        .lock()
        .map_err(|_| unavailable_state())?
        .as_ref()
        .map(|slot| embedding_sync::stored_space_fingerprint(slot.provider.space()))
        .transpose()?;
    evidence::status(&*state.index()?, &workspace_id, loaded_space.as_deref())
}

/// "Prepare now": brings the index up to date (an incremental scan, then an
/// embedding fill for the chunks that lack a vector) and reports what it holds.
#[tauri::command]
async fn rebuild_index(
    app: AppHandle,
    state: State<'_, Folio>,
    embedding_state: State<'_, EmbeddingState>,
    lab_state: State<'_, lab_commands::LabState>,
    workspace_id: String,
) -> Result<evidence::IndexStatus, FolioError> {
    let request = AiRequest::new(
        &app,
        state.inner(),
        embedding_state.inner(),
        lab_state.inner(),
        &workspace_id,
    )?;
    Ok(run_blocking::<_, FolioError, _>(move || request.run(|index| index.prepare())).await?)
}

/// Hybrid semantic search over the persistent index, or FTS5 keyword search
/// (labelled `keyword`) when no embedding model is selected. Brings the index
/// up to date first; a Model Lab run makes it `providerBusy`.
#[tauri::command]
async fn semantic_search(
    app: AppHandle,
    state: State<'_, Folio>,
    embedding_state: State<'_, EmbeddingState>,
    lab_state: State<'_, lab_commands::LabState>,
    workspace_id: String,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<ProviderSearchResult>, FolioError> {
    let request = AiRequest::new(
        &app,
        state.inner(),
        embedding_state.inner(),
        lab_state.inner(),
        &workspace_id,
    )?;
    Ok(run_blocking::<_, FolioError, _>(move || {
        request.run(|index| {
            index.search(
                &query,
                evidence::Scope::Folder,
                limit.unwrap_or(10).clamp(1, 50),
            )
        })
    })
    .await?)
}

/// Holds the generation slot for as long as this value lives. `Drop` releases
/// it through the same `finish_generation` handshake every holder uses
/// (#93), on every exit path — an early `?`, a normal return, or a panic
/// unwinding through `spawn_blocking` — so a caller can never forget to
/// release it, and a crash mid-request can never leave Folio stuck "busy".
struct SlotClaim {
    generation_state: GenerationState,
    cancel: Arc<AtomicBool>,
}

impl SlotClaim {
    /// Marks the slot active. The caller must already hold the lock and have
    /// checked `ensure_slot_free`.
    fn new(guard: &mut GenerationStateInner, generation_state: &GenerationState) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        guard.active_cancel = Some(cancel.clone());
        Self {
            generation_state: generation_state.clone(),
            cancel,
        }
    }
}

impl Drop for SlotClaim {
    fn drop(&mut self) {
        let _ = finish_generation(&self.generation_state, &self.cancel);
    }
}

struct GenerationLease {
    provider: Arc<LlamaServerProvider>,
    claim: SlotClaim,
}

fn ensure_slot_free(guard: &GenerationStateInner) -> Result<(), NativeProviderError> {
    if guard.active_cancel.is_some() {
        return Err(NativeProviderError {
            code: folio_core::contracts::ProviderErrorCode::GenerationBusy,
            message: "Another local generation request is active.".into(),
            detail: None,
        });
    }
    if guard.unloading > 0 {
        return Err(NativeProviderError {
            code: folio_core::contracts::ProviderErrorCode::GenerationBusy,
            message: "Folio is stopping the local AI model. Try again in a moment.".into(),
            detail: None,
        });
    }
    if guard.runtime_installing {
        return Err(NativeProviderError {
            code: folio_core::contracts::ProviderErrorCode::GenerationBusy,
            message: "The local AI runtime is being installed. Try again when it finishes.".into(),
            detail: None,
        });
    }
    Ok(())
}

/// `acquire_generation`'s slot handling without the model store or provider
/// launch, so tests can exercise the busy check and release path directly.
#[cfg(test)]
fn claim_free_slot(generation_state: &GenerationState) -> Result<SlotClaim, NativeProviderError> {
    let mut guard = generation_state.lock().unwrap();
    ensure_slot_free(&guard)?;
    Ok(SlotClaim::new(&mut guard, generation_state))
}

/// The generation provider for the selected model, marked active in the same
/// critical section. A model switch, removal or runtime reinstall that runs
/// afterwards therefore sees this request and cancels it (or refuses), instead
/// of unloading a provider that this request then starts again outside the slot.
fn acquire_generation(
    app: &AppHandle,
    generation_state: &GenerationState,
) -> Result<GenerationLease, NativeProviderError> {
    let store = model_store(app)?;
    let model_id = store
        .selected_model(ModelRole::Generation)
        .map_err(native_error)?
        .ok_or_else(|| NativeProviderError {
            code: folio_core::contracts::ProviderErrorCode::ModelNotInstalled,
            message: "Select a verified local generation model first.".into(),
            detail: None,
        })?;
    let verified = store.verified_model_file(&model_id).map_err(native_error)?;
    // Verified against the install record before every launch.
    let executable = store
        .verified_runtime_executable(runtime_id_for_host())
        .map_err(native_error)?;
    let mut guard = generation_state.lock().map_err(|_| NativeProviderError {
        code: folio_core::contracts::ProviderErrorCode::IoError,
        message: "The local generation state is unavailable.".into(),
        detail: None,
    })?;
    ensure_slot_free(&guard)?;
    if let Some(slot) = guard.slot.as_ref() {
        if slot.model_id == verified.descriptor.id && slot.revision == verified.descriptor.revision
        {
            let provider = slot.provider.clone();
            let claim = SlotClaim::new(&mut guard, generation_state);
            return Ok(GenerationLease { provider, claim });
        }
    }
    if let Some(slot) = guard.slot.take() {
        slot.provider.unload().map_err(native_error)?;
    }
    let threads = std::thread::available_parallelism()
        .map(|value| value.get().saturating_sub(1).max(1))
        .unwrap_or(1);
    let provider = Arc::new(
        LlamaServerProvider::from_verified_model(executable, verified, threads)
            .map_err(native_error)?,
    );
    guard.slot = Some(GenerationSlot {
        model_id: provider.model_id().into(),
        revision: provider.revision().into(),
        provider: provider.clone(),
    });
    let claim = SlotClaim::new(&mut guard, generation_state);
    Ok(GenerationLease { provider, claim })
}

fn finish_generation(
    generation_state: &GenerationState,
    cancel: &Arc<AtomicBool>,
) -> Result<(), NativeProviderError> {
    let mut guard = generation_state.lock().map_err(|_| NativeProviderError {
        code: folio_core::contracts::ProviderErrorCode::IoError,
        message: "The local generation state is unavailable.".into(),
        detail: None,
    })?;
    if guard
        .active_cancel
        .as_ref()
        .is_some_and(|active| Arc::ptr_eq(active, cancel))
    {
        guard.active_cancel = None;
    }
    Ok(())
}

/// Asks the holder of the slot to stop. The slot stays held until the holder
/// releases it, so a new request cannot start a second server meanwhile.
fn signal_holder_stop(guard: &GenerationStateInner) -> Result<(), NativeProviderError> {
    if let Some(cancel) = guard.active_cancel.as_ref() {
        cancel.store(true, Ordering::Release);
        if let Some(slot) = guard.slot.as_ref() {
            slot.provider.cancel_active().map_err(native_error)?;
        }
    }
    Ok(())
}

fn cancel_generation_now(generation_state: &GenerationState) -> Result<(), NativeProviderError> {
    let guard = generation_state.lock().map_err(|_| NativeProviderError {
        code: folio_core::contracts::ProviderErrorCode::IoError,
        message: "The local generation state is unavailable.".into(),
        detail: None,
    })?;
    signal_holder_stop(&guard)
}

#[tauri::command]
fn cancel_generation(
    state: State<'_, Folio>,
    generation_state: State<'_, GenerationState>,
) -> Result<(), FolioError> {
    // The request may still be reading or embedding, before it holds the slot.
    state.cancel_ai_request.store(true, Ordering::Release);
    Ok(cancel_generation_now(generation_state.inner())?)
}

#[tauri::command]
async fn summarize_document(
    app: AppHandle,
    state: State<'_, Folio>,
    generation_state: State<'_, GenerationState>,
    workspace_id: String,
    document_id: String,
) -> Result<GroundedResult, FolioError> {
    let relative_path = ai_boundary::parse_document_id(&workspace_id, &document_id)?;
    let root = ai_boundary::resolve_workspace(state.inner(), &workspace_id)?;
    let generation_state = generation_state.inner().clone();
    Ok(run_blocking::<_, FolioError, _>(move || {
        let document_text = read_ai_document(&root, &relative_path)?;
        let content = document_text.content.clone();
        let passages =
            grounding::summary_passages(&document_id, &content, &document_text.content_hash);
        let lease = acquire_generation(&app, &generation_state)?;
        let result = grounding::summarize_document(
            lease.provider.as_ref(),
            passages,
            grounding::detect_language(&content),
            lease.claim.cancel.as_ref(),
        );
        Ok(result?)
    })
    .await?)
}

/// Answers from passages of the persistent index (all of the folder, or one
/// document when `document_id` is given). With no evidence the model is never
/// called.
#[tauri::command]
async fn answer_question(
    app: AppHandle,
    state: State<'_, Folio>,
    embedding_state: State<'_, EmbeddingState>,
    lab_state: State<'_, lab_commands::LabState>,
    generation_state: State<'_, GenerationState>,
    workspace_id: String,
    question: String,
    document_id: Option<String>,
) -> Result<GroundedResult, FolioError> {
    let document_id = ai_boundary::validate_document_filter(&workspace_id, document_id.as_deref())?;
    let request = AiRequest::new(
        &app,
        state.inner(),
        embedding_state.inner(),
        lab_state.inner(),
        &workspace_id,
    )?;
    let generation_state = generation_state.inner().clone();
    Ok(run_blocking::<_, FolioError, _>(move || {
        let cancel = request.cancel.clone();
        let passages = request.run(|index| {
            index.prompt_evidence(
                &question,
                match document_id.as_deref() {
                    Some(id) => evidence::Scope::Document(id),
                    None => evidence::Scope::Folder,
                },
            )
        })?;
        if passages.is_empty() {
            return Ok(grounding::answer_question(
                None,
                &question,
                passages,
                grounding::detect_language(&question),
                &AtomicBool::new(false),
            )?);
        }
        let lease = acquire_generation(&app, &generation_state)?;
        if cancel.load(Ordering::Acquire) {
            return Err(error(ErrorCode::Cancelled, "The request was cancelled."));
        }
        let result = grounding::answer_question(
            Some(lease.provider.as_ref()),
            &question,
            passages,
            grounding::detect_language(&question),
            lease.claim.cancel.as_ref(),
        );
        Ok(result?)
    })
    .await?)
}

/// Interprets a request. The model sees the request only; the folder's index is
/// brought up to date, and then only the few files the target description could
/// mean are read, to resolve it. `document_id` is a file the user picked or
/// attached: a change then targets it directly.
#[tauri::command]
async fn interpret_request(
    app: AppHandle,
    state: State<'_, Folio>,
    embedding_state: State<'_, EmbeddingState>,
    lab_state: State<'_, lab_commands::LabState>,
    generation_state: State<'_, GenerationState>,
    workspace_id: String,
    text: String,
    document_id: Option<String>,
) -> Result<InterpretationResult, FolioError> {
    let document_id = ai_boundary::validate_document_filter(&workspace_id, document_id.as_deref())?;
    let request = AiRequest::new(
        &app,
        state.inner(),
        embedding_state.inner(),
        lab_state.inner(),
        &workspace_id,
    )?;
    let generation_state = generation_state.inner().clone();
    Ok(run_blocking::<_, FolioError, _>(move || {
        request.run(|index| {
            // Files first, so the slot isn't held while the index catches up.
            index.refresh_files()?;
            let lease = acquire_generation(&app, &generation_state)?;
            // A cancel pressed while the model was being verified and started.
            index.ensure_not_cancelled()?;
            let generated = interpretation::generate_intent(
                lease.provider.as_ref(),
                &text,
                lease.claim.cancel.as_ref(),
            )?;
            drop(lease);
            let intent = match generated.intent {
                Ok(intent) => intent,
                Err(invalid) => return Ok(invalid),
            };
            let corpus = index.interpretation_corpus(
                intent.target_description.as_deref(),
                document_id.as_deref(),
            )?;
            Ok(interpretation::resolve_model_intent_for(
                &intent,
                grounding::detect_language(&text),
                &corpus.documents,
                &corpus.contents,
                &corpus.chunks,
                document_id.as_deref(),
            ))
        })
    })
    .await?)
}

/// Signals whoever holds the generation slot (an ordinary request or a
/// Model Lab run — both set `active_cancel`) to stop, and waits up to 10
/// seconds (the same bound the exit hook gives a lab run) for them to
/// release it through the `finish_generation`/`finish_lab` handshake every
/// holder already uses, before unloading the parked provider. The previous
/// version cleared `active_cancel` and the slot immediately: the slot looked
/// free, and a new request's `acquire_generation` could start a second
/// `llama-server` while the first was still winding down (#93), or the
/// unload could kill the holder's server out from under its still-running
/// request. If the holder hasn't released it within the bound, this reports
/// busy rather than unloading a server something may still be using.
fn unload_generation_now(generation_state: &GenerationState) -> Result<(), NativeProviderError> {
    unload_generation_now_with_limit(generation_state, Duration::from_secs(10))
}

/// At app exit: wait like `unload_generation_now`, then stop the server even
/// if its holder never released the slot. Nothing can use it after exit, and
/// on macOS and Linux nothing else stops the child process.
fn unload_generation_at_exit(generation_state: &GenerationState, limit: Duration) {
    if unload_generation_now_with_limit(generation_state, limit).is_ok() {
        return;
    }
    let slot = generation_state.lock().ok().and_then(|mut guard| guard.slot.take());
    if let Some(slot) = slot {
        let _ = slot.provider.unload();
    }
}

/// Separated from `unload_generation_now` only so tests can use a short
/// limit instead of waiting the real 10 seconds.
fn unload_generation_now_with_limit(
    generation_state: &GenerationState,
    limit: Duration,
) -> Result<(), NativeProviderError> {
    let deadline = Instant::now() + limit;
    let unavailable = || NativeProviderError {
        code: folio_core::contracts::ProviderErrorCode::IoError,
        message: "The local generation state is unavailable.".into(),
        detail: None,
    };
    let _unloading = {
        let mut guard = generation_state.lock().map_err(|_| unavailable())?;
        UnloadingMark::new(&mut guard, generation_state)
    };
    loop {
        let mut guard = generation_state.lock().map_err(|_| unavailable())?;
        if let Some(cancel) = guard.active_cancel.as_ref() {
            // As `cancel_generation` does: the flag, then interrupt a holder
            // blocked waiting on llama-server, which only checks the flag once
            // a read returns. The slot stays claimed until the holder releases
            // it, so no second server can start meanwhile.
            cancel.store(true, Ordering::Release);
            if let Some(slot) = guard.slot.as_ref() {
                let _ = slot.provider.cancel_active();
            }
        } else {
            if let Some(slot) = guard.slot.take() {
                drop(guard);
                slot.provider.unload().map_err(native_error)?;
            }
            return Ok(());
        }
        drop(guard);
        if Instant::now() >= deadline {
            return Err(NativeProviderError {
                code: folio_core::contracts::ProviderErrorCode::GenerationBusy,
                message: "Another local generation request is still finishing.".into(),
                detail: None,
            });
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn exit_does_not_wait_forever_for_a_request_that_never_releases_the_slot() {
        let generation = GenerationState::default();
        let _held = claim_free_slot(&generation).unwrap();
        let started = Instant::now();
        unload_generation_at_exit(&generation, Duration::from_millis(60));
        assert!(started.elapsed() < Duration::from_secs(5));
        // The slot itself was taken for unloading even though the claim is held.
        assert!(generation.lock().unwrap().slot.is_none());
    }

    #[test]
    fn unloading_during_a_request_waits_for_the_request_to_release_the_slot() {
        let generation = GenerationState::default();
        let claim = claim_free_slot(&generation).unwrap();
        let cancel = claim.cancel.clone();
        let (release, released) = std::sync::mpsc::channel::<()>();
        let request = std::thread::spawn(move || {
            // Stand in for a request that notices cancellation and returns,
            // only once the test has checked the slot is still held.
            while !claim.cancel.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(5));
            }
            released.recv().unwrap();
            drop(claim);
        });
        let generation_for_unload = generation.clone();
        let unloader = std::thread::spawn(move || unload_generation_now(&generation_for_unload));
        while !cancel.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(claim_free_slot(&generation).is_err());
        release.send(()).unwrap();
        unloader.join().unwrap().unwrap();
        request.join().unwrap();
        assert!(generation.lock().unwrap().active_cancel.is_none());
        assert!(claim_free_slot(&generation).is_ok());
    }

    #[test]
    fn no_request_slips_in_between_a_release_and_the_unload() {
        let generation = GenerationState::default();
        let claim = claim_free_slot(&generation).unwrap();
        let cancel = claim.cancel.clone();
        let generation_for_unload = generation.clone();
        let unloader = std::thread::spawn(move || unload_generation_now(&generation_for_unload));
        while !cancel.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(5));
        }
        // While the unload waits, the slot is marked, and a marked slot is
        // refused even with no holder, so between the holder's release and
        // the unload's next look nothing can claim it.
        assert_eq!(generation.lock().unwrap().unloading, 1);
        let between = GenerationStateInner {
            unloading: 1,
            ..GenerationStateInner::default()
        };
        assert_eq!(
            ensure_slot_free(&between).err().map(|failure| failure.code),
            Some(folio_core::contracts::ProviderErrorCode::GenerationBusy)
        );
        drop(claim);
        unloader.join().unwrap().unwrap();
        assert_eq!(generation.lock().unwrap().unloading, 0);
        assert!(claim_free_slot(&generation).is_ok());
    }

    #[test]
    fn the_slot_is_released_when_a_request_fails_or_panics() {
        let generation = GenerationState::default();
        let failing_request = |state: &GenerationState| -> Result<(), NativeProviderError> {
            let _claim = claim_free_slot(state)?;
            Err(NativeProviderError {
                code: folio_core::contracts::ProviderErrorCode::IoError,
                message: "generation failed".into(),
                detail: None,
            })
        };
        assert!(failing_request(&generation).is_err());
        assert!(generation.lock().unwrap().active_cancel.is_none());

        let generation_for_panic = generation.clone();
        let panicked = std::thread::spawn(move || {
            let _claim = claim_free_slot(&generation_for_panic).unwrap();
            panic!("request panicked while holding the slot");
        })
        .join();
        assert!(panicked.is_err());
        assert!(claim_free_slot(&generation).is_ok());
    }











    #[test]
    fn relationship_passages_need_current_hash_utf8_boundaries_and_exact_bytes() {
        let current = DocumentText {
            content: "aé b".into(),
            content_hash: "sha256:current".into(),
            size_bytes: 5,
            modified_at_ms: None,
            pages: Vec::new(),
            unreadable_pages: Vec::new(),
        };
        let valid = CoreSourcePassage {
            document_id: "workspace:notes.md".into(),
            document_content_hash: current.content_hash.clone(),
            offset_unit: folio_core::contracts::OffsetUnit::Utf8Byte,
            start: 1,
            end: 3,
            text: "é".into(),
            page: None,
        };
        assert!(relationship_passage_is_current(&valid, &current));
        assert!(!relationship_passage_is_current(
            &CoreSourcePassage {
                start: 2,
                end: 3,
                ..valid.clone()
            },
            &current,
        ));
        assert!(!relationship_passage_is_current(
            &CoreSourcePassage {
                text: "x".into(),
                ..valid.clone()
            },
            &current,
        ));
        assert!(!relationship_passage_is_current(
            &CoreSourcePassage {
                document_content_hash: "sha256:old".into(),
                ..valid
            },
            &current,
        ));
    }

    #[test]
    fn ai_document_reads_preserve_native_path_escape_errors() {
        let parent = tempfile::tempdir().unwrap();
        let root_path = parent.path().join("workspace");
        fs::create_dir(&root_path).unwrap();
        fs::write(parent.path().join("outside.md"), "outside").unwrap();
        let root = ScopedRoot {
            id: "workspace".into(),
            path: root_path,
        };

        let failure = read_ai_document(&root, "../outside.md").unwrap_err();
        assert_eq!(failure.code, ErrorCode::PathEscapesWorkspace);
    }

    #[test]
    fn an_unreadable_model_store_means_links_only_not_a_refused_preview() {
        let unreadable = Err(error(ErrorCode::Internal, "settings.json could not be parsed"));
        assert!(lenient_descriptor(unreadable).is_none());
        assert!(lenient_descriptor(Ok(None)).is_none());
    }

    #[test]
    fn a_refresh_leaves_out_the_phases_that_did_not_run() {
        let refresh = LocalAiRefresh {
            workspace_id: "w".into(),
            embedding: None,
            discovery: None,
            ended: None,
            coverage: ai_discovery::RelationshipCoverage {
                state: ai_discovery::CoverageState::NoActiveSpace,
                space_fingerprint: None,
                eligible_documents: 0,
                indexed_documents: 0,
                pairs_considered: 0,
                pairs_remaining: 0,
                overflow_documents: 0,
            },
        };
        let wire = serde_json::to_value(&refresh).unwrap();
        let keys: Vec<&str> = wire.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, ["coverage", "workspaceId"], "absent, never null: {wire}");
    }

    #[test]
    fn every_managed_state_has_its_own_type() {
        use std::any::TypeId;
        // Tauri keeps one managed state per type, and a second `.manage` of the
        // same type panics before any window opens. Keep this list in step
        // with `run()` and its `setup`.
        let managed = [
            ("EmbeddingState", TypeId::of::<EmbeddingState>()),
            ("GenerationState", TypeId::of::<GenerationState>()),
            ("InstallState", TypeId::of::<InstallState>()),
            ("LabState", TypeId::of::<lab_commands::LabState>()),
            ("Folio", TypeId::of::<Folio>()),
        ];
        for (index, (name, id)) in managed.iter().enumerate() {
            for (other, other_id) in &managed[index + 1..] {
                assert_ne!(id, other_id, "{name} and {other} are the same type");
            }
        }
    }

    #[test]
    fn persistent_embedding_sync_refuses_an_active_model_lab_run() {
        let lab_state = lab_commands::LabState::default();
        assert!(refuse_during_lab(&lab_state).is_ok());

        *lab_state.lock().unwrap() = Some(Arc::new(AtomicBool::new(false)));
        let failure = refuse_during_lab(&lab_state).unwrap_err();
        assert_eq!(failure.code, ErrorCode::ProviderBusy);
        assert_eq!(failure.detail("reason"), Some("modelLabRunning"));
    }

    #[test]
    fn guarded_embedding_state_refuses_lab_before_load_work() {
        let embedding_state = EmbeddingState::default();
        let lab_state = lab_commands::LabState::default();
        *lab_state.lock().unwrap() = Some(Arc::new(AtomicBool::new(false)));
        let load_attempted = Arc::new(AtomicBool::new(false));
        let load_attempted_for_work = load_attempted.clone();

        let failure = with_embedding_state_guarded(
            &embedding_state,
            || refuse_during_lab(&lab_state),
            |_slot| {
                load_attempted_for_work.store(true, Ordering::Release);
                Ok(())
            },
        )
        .unwrap_err();

        assert_eq!(failure.code, ErrorCode::ProviderBusy);
        assert_eq!(failure.detail("reason"), Some("modelLabRunning"));
        assert!(!load_attempted.load(Ordering::Acquire));
    }
}

#[tauri::command]
async fn unload_generation(generation_state: State<'_, GenerationState>) -> Result<(), FolioError> {
    let generation_state = generation_state.inner().clone();
    Ok(run_blocking(move || unload_generation_now(&generation_state)).await?)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(EmbeddingState::default())
        .manage(GenerationState::default())
        .manage(InstallState::default())
        // Each managed state must be its own type (see
        // `every_managed_state_has_its_own_type`).
        .manage(lab_commands::LabState::default())
        .setup(|app| {
            let directory = app.path().app_data_dir()?;
            std::fs::create_dir_all(&directory)?;
            let folio = Folio::open(directory.join("folio.sqlite"))?;
            lab_commands::mark_interrupted_runs(&folio);
            app.manage(folio);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            choose_workspace,
            list_workspaces,
            reopen_workspace,
            list_documents,
            read_document,
            scan_workspace,
            recheck_documents,
            cancel_indexing,
            list_indexed_documents,
            search_index,
            list_duplicates,
            list_relationships,
            refresh_ai_connections,
            cancel_ai_connections,
            register_embedding_space,
            pending_embedding_chunks,
            put_embeddings,
            vector_candidates,
            sync_embeddings,
            cancel_embedding_sync,
            refresh_local_ai_index,
            cancel_local_ai_refresh,
            relationship_coverage,
            prepare_plan,
            approve_plan,
            apply_plan,
            cancel_apply,
            preview_undo,
            undo_plan,
            list_history,
            list_activity,
            ripple_impacts,
            prepare_passage_edit,
            organization_suggestions,
            list_models,
            verify_model,
            install_model,
            remove_model,
            select_model,
            runtime_status,
            install_runtime,
            cancel_install,
            model_setup,
            rebuild_index,
            index_status,
            semantic_search,
            summarize_document,
            summarize_relationships,
            explain_impact,
            answer_question,
            interpret_request,
            cancel_generation,
            unload_generation,
            lab_commands::lab_models,
            lab_commands::install_lab_candidate,
            lab_commands::remove_lab_candidate,
            lab_commands::verify_lab_candidate,
            lab_commands::run_model_lab,
            lab_commands::cancel_model_lab,
            lab_commands::list_lab_results,
            lab_commands::list_lab_runs,
            lab_commands::record_lab_review
        ])
        .build(tauri::generate_context!())
        .expect("Folio could not start");
    app.run(|app_handle, event| {
        if matches!(
            event,
            tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
        ) {
            // Both events fire; the waiting happens once.
            static EXIT_UNLOADED: AtomicBool = AtomicBool::new(false);
            if EXIT_UNLOADED.swap(true, Ordering::SeqCst) {
                return;
            }
            if let Some(generation_state) = app_handle.try_state::<GenerationState>() {
                unload_generation_at_exit(generation_state.inner(), Duration::from_secs(10));
            }
            // A lab run's server lives in the run's thread, outside the slot.
            if let Some(lab_state) = app_handle.try_state::<lab_commands::LabState>() {
                let _ = lab_commands::stop_lab_and_wait(
                    lab_state.inner(),
                    std::time::Duration::from_secs(10),
                );
            }
        }
    });
}

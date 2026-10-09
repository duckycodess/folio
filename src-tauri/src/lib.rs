mod ai_boundary;
mod config_guard;
mod contract_fixtures;
mod contracts;
mod db;
mod embedding_sync;
mod error;
mod extract;
mod identity;
mod index;
mod lab_commands;
mod lab_store;
mod organize;
mod plan;
mod ripple;
mod workspace;
mod writer;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use folio_core::chunking::{Chunk, InterimTextChunker, TextDocument};
use folio_core::contracts::{
    DocumentRecord, EmbeddingSpace as ProviderEmbeddingSpace, GroundedResult,
    InterpretationResult, Language, ModelDescriptor, ModelInstallState, ModelInstallStatus,
    ModelRole, NativeProviderError, SearchResult as ProviderSearchResult,
};
use folio_core::embeddings::{EmbeddingKind, EmbeddingProvider, OrtE5Provider};
use folio_core::error::CoreError;
use folio_core::generation::{GenerationProvider, LlamaServerProvider};
use folio_core::grounding;
use folio_core::interpretation;
use folio_core::models::{DownloadProgress, ModelStore, RuntimeStatus};
use folio_core::retrieval::HybridRetriever;
use rusqlite::Connection;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

use contracts::{ActionPlan, ActivityBatch, Approval, FileOperation, HistoryEntry, ImpactCandidate, PlanSource, UndoPreflight};
use error::{error, ErrorCode, FolioError};
use index::{
    ChunkVector, DuplicateGroup, EmbeddingSpace, ExplicitReference, IndexProgress,
    IndexedDocument, PendingChunk, ScanOptions, ScanSummary, SearchResult, VectorCandidate,
};
use organize::OrganizationSuggestions;
use plan::PlanRegistry;
use identity::media_type_for_path;
use writer::{ApplyReport, RealFileSystem, UndoReport};
use workspace::{
    DocumentListing, DocumentText, KnownWorkspace, ScopedRoot, WorkspaceInfo, WorkspaceRegistry,
};

const INDEX_PROGRESS_EVENT: &str = "folio://index-progress";

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
    /// Stops an apply before its next operation; the running one finishes.
    cancel_apply: Arc<AtomicBool>,
    /// Serializes persistent embedding fills without holding the index or
    /// provider lock across the whole run.
    embedding_sync: Arc<Mutex<()>>,
    cancel_embedding_sync: Arc<AtomicBool>,
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
            cancel_apply: Arc::new(AtomicBool::new(false)),
            embedding_sync: Arc::new(Mutex::new(())),
            cancel_embedding_sync: Arc::new(AtomicBool::new(false)),
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
    index_state: State<'_, IndexState>,
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
    // The issue #4 provider snapshot is rebuilt from current files on demand.
    *index_state.lock().map_err(|_| unavailable_state())? = None;
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
    state: State<'_, Folio>,
    workspace_id: String,
) -> Result<Vec<ExplicitReference>, FolioError> {
    state.root(&workspace_id)?;
    index::list_relationships(&*state.index()?, &workspace_id)
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

        let conn = db::open(&index_path)?;
        let space_fingerprint = index::register_space(&conn, &stored_space)?;
        let mut store = embedding_sync::IndexChunkStore::new(
            conn,
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
            &cancel,
            embedding_sync::SyncLimits::default(),
        )
    })
    .await?)
}

#[tauri::command]
fn cancel_embedding_sync(state: State<'_, Folio>) {
    state.cancel_embedding_sync.store(true, Ordering::Release);
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
    let workspaces = state.workspaces.lock().map_err(|_| unavailable_state())?;
    let root = workspaces.resolve(&workspace_id)?;
    // Ripple evidence comes from the index and each edit's diff unless the caller
    // supplies it (for example with the exact phrase an interpreter replaced).
    let impacts = match impacts {
        Some(impacts) => impacts,
        None => ripple::plan_impacts(&*state.index()?, &root, &operations)?,
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
    state: State<'_, Folio>,
    workspace_id: String,
    document_id: String,
    replaced_text: String,
) -> Result<Vec<ImpactCandidate>, FolioError> {
    state.root(&workspace_id)?;
    let index = state.index()?;
    let document = index::get_document(&index, &workspace_id, &document_id)?;
    ripple::impacts(&index, &workspace_id, &document, &replaced_text)
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

#[derive(Clone)]
struct IndexSnapshot {
    workspace_id: String,
    /// Path, size and modification time of every text document the snapshot
    /// was built from. A different listing means the files changed (an
    /// approved edit, an undo, a scan or an external change), so the snapshot
    /// is rebuilt instead of citing old text.
    source_fingerprint: Vec<(String, u64, Option<u64>)>,
    documents: Vec<DocumentRecord>,
    chunks: Vec<Chunk>,
    retriever: HybridRetriever,
    embedding_space: Option<ProviderEmbeddingSpace>,
    skipped_documents: Vec<SkippedDocument>,
}

/// Shared, not cloned per query.
type IndexState = Arc<Mutex<Option<Arc<IndexSnapshot>>>>;

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
}

type GenerationState = Arc<Mutex<GenerationStateInner>>;
type InstallState = Arc<Mutex<Option<Arc<AtomicBool>>>>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IndexStatus {
    workspace_id: Option<String>,
    document_count: usize,
    chunk_count: usize,
    method: String,
    space_fingerprint: Option<String>,
    skipped_documents: Vec<SkippedDocument>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkippedDocument {
    relative_path: String,
    reason: String,
}

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

fn invalidate_index(index_state: &IndexState) -> Result<(), NativeProviderError> {
    *index_state.lock().map_err(|_| NativeProviderError {
        code: folio_core::contracts::ProviderErrorCode::IoError,
        message: "The local index state is unavailable.".into(),
        detail: None,
    })? = None;
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

#[tauri::command]
async fn verify_model(
    app: AppHandle,
    model_id: String,
) -> Result<ProviderInstallState, FolioError> {
    let state = run_blocking(move || {
        model_store(&app)?
            .verify_model(&model_id)
            .map_err(native_error)
    })
    .await?;
    Ok(provider_install_state(state))
}

#[tauri::command]
async fn install_model(
    app: AppHandle,
    index_state: State<'_, IndexState>,
    embedding_state: State<'_, EmbeddingState>,
    install_state: State<'_, InstallState>,
    model_id: String,
) -> Result<ProviderInstallState, FolioError> {
    let cancel = begin_install(install_state.inner())?;
    let worker_cancel = cancel.clone();
    let install_state = install_state.inner().clone();
    let index_state = index_state.inner().clone();
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
    invalidate_index(&index_state)?;
    Ok(provider_install_state(result?))
}

#[tauri::command]
async fn remove_model(
    app: AppHandle,
    index_state: State<'_, IndexState>,
    embedding_state: State<'_, EmbeddingState>,
    generation_state: State<'_, GenerationState>,
    install_state: State<'_, InstallState>,
    model_id: String,
) -> Result<(), FolioError> {
    let index_state = index_state.inner().clone();
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
    invalidate_index(&index_state)?;
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
    index_state: State<'_, IndexState>,
    embedding_state: State<'_, EmbeddingState>,
    install_state: State<'_, InstallState>,
    role: ModelRole,
    model_id: String,
) -> Result<(), FolioError> {
    let embedding_selection = matches!(&role, ModelRole::Embedding);
    let index_state = index_state.inner().clone();
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
    if embedding_selection {
        invalidate_index(&index_state)?;
    }
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

fn load_corpus(
    root: &ScopedRoot,
) -> Result<
    (
        Vec<DocumentRecord>,
        HashMap<String, String>,
        Vec<Chunk>,
        Vec<SkippedDocument>,
    ),
    FolioError,
> {
    let metadata = workspace::list_documents(root)?.documents;
    let mut documents = Vec::new();
    let mut contents = HashMap::new();
    let mut text_documents = Vec::new();
    let mut skipped_documents = Vec::new();
    for row in metadata {
        let extension = Path::new(&row.relative_path)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !matches!(extension.as_str(), "txt" | "md") {
            continue;
        }
        let document_text = match workspace::read_text(&root.path, &row.relative_path) {
            Ok(content) => content,
            Err(reason) => {
                skipped_documents.push(SkippedDocument {
                    relative_path: row.relative_path,
                    reason: reason.to_string(),
                });
                continue;
            }
        };
        let content = document_text.content.clone();
        let record = DocumentRecord {
            id: row.id.clone(),
            workspace_id: row.workspace_id.clone(),
            relative_path: row.relative_path.clone(),
            name: row.name.clone(),
            title: folio_core::embeddings::markdown_title(&row.name, &content),
            language: Language::Unknown,
            media_type: row.media_type.clone(),
            size_bytes: document_text.size_bytes,
            modified_at_ms: document_text.modified_at_ms.or(row.modified_at_ms),
            content: Some(content.clone()),
            content_hash: Some(document_text.content_hash),
        };
        contents.insert(record.id.clone(), content.clone());
        text_documents.push(TextDocument::new(record.clone(), content));
        documents.push(record);
    }
    let chunks = InterimTextChunker::new(text_documents).all_chunks()?;
    Ok((documents, contents, chunks, skipped_documents))
}

fn document_record(
    root: &ScopedRoot,
    document_id: &str,
    relative_path: &str,
    document_text: &DocumentText,
    content: &str,
) -> DocumentRecord {
    DocumentRecord {
        id: document_id.into(),
        workspace_id: root.id.clone(),
        relative_path: relative_path.into(),
        name: Path::new(relative_path)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or(relative_path)
            .into(),
        title: folio_core::embeddings::markdown_title(relative_path, content),
        language: grounding::detect_language(content),
        media_type: media_type_for_path(relative_path)
            .unwrap_or("text/plain")
            .into(),
        size_bytes: document_text.size_bytes,
        modified_at_ms: document_text.modified_at_ms,
        content: Some(content.into()),
        content_hash: Some(document_text.content_hash.clone()),
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
            let model_file = descriptor
                .files
                .iter()
                .find(|file| file.path.ends_with(".onnx"))
                .ok_or_else(|| NativeProviderError {
                    code: folio_core::contracts::ProviderErrorCode::ModelCorrupt,
                    message: "The selected embedding model has no ONNX file.".into(),
                    detail: Some(model_id.clone()),
                })?;
            let tokenizer_file = descriptor
                .files
                .iter()
                .find(|file| file.path.ends_with("tokenizer.json"))
                .ok_or_else(|| NativeProviderError {
                    code: folio_core::contracts::ProviderErrorCode::ModelCorrupt,
                    message: "The selected embedding model has no tokenizer file.".into(),
                    detail: Some(model_id.clone()),
                })?;
            let model_path = store
                .verified_file_path(&model_id, &model_file.path)
                .map_err(native_error)?;
            let tokenizer_path = store
                .verified_file_path(&model_id, &tokenizer_file.path)
                .map_err(native_error)?;
            let provider = OrtE5Provider::from_files(
                model_path,
                tokenizer_path,
                descriptor.id.clone(),
                descriptor.revision.clone(),
                descriptor.quantization.clone(),
                384,
                &model_file.sha256,
                &tokenizer_file.sha256,
                folio_core::embeddings::DEFAULT_MAX_TOKENS,
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

fn with_embedding_provider<T, F>(
    app: &AppHandle,
    embedding_state: &EmbeddingState,
    work: F,
) -> Result<Option<T>, FolioError>
where
    F: FnOnce(&OrtE5Provider) -> Result<T, NativeProviderError>,
{
    with_embedding_provider_guarded(app, embedding_state, || Ok(()), work)
}

/// The text documents the provider snapshot reads, as (path, size, mtime).
fn corpus_fingerprint(root: &ScopedRoot) -> Result<Vec<(String, u64, Option<u64>)>, FolioError> {
    let mut fingerprint = workspace::list_documents(root)?
        .documents
        .into_iter()
        .filter(|row| {
            let extension = Path::new(&row.relative_path)
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            matches!(extension.as_str(), "txt" | "md")
        })
        .map(|row| (row.relative_path, row.size_bytes, row.modified_at_ms))
        .collect::<Vec<_>>();
    fingerprint.sort();
    Ok(fingerprint)
}

fn build_snapshot(
    app: &AppHandle,
    embedding_state: &EmbeddingState,
    root: &ScopedRoot,
) -> Result<IndexSnapshot, FolioError> {
    let source_fingerprint = corpus_fingerprint(root)?;
    let (documents, _contents, chunks, skipped_documents) = load_corpus(root)?;
    let mut retriever = HybridRetriever::default();
    let mut embedding_space = None;
    if let Some((space, vectors)) = with_embedding_provider(app, embedding_state, |provider| {
        let texts = folio_core::embeddings::passage_embedding_texts(&documents, &chunks);
        let vectors = provider
            .embed(&texts, EmbeddingKind::Passage, None)
            .map_err(native_error)?;
        Ok((provider.space().clone(), vectors))
    })? {
        retriever
            .vector_index
            .replace(space.clone(), chunks.clone(), vectors)?;
        embedding_space = Some(space);
    }
    Ok(IndexSnapshot {
        workspace_id: root.id.clone(),
        source_fingerprint,
        documents,
        chunks,
        retriever,
        embedding_space,
        skipped_documents,
    })
}

#[tauri::command]
async fn rebuild_index(
    app: AppHandle,
    state: State<'_, Folio>,
    index_state: State<'_, IndexState>,
    embedding_state: State<'_, EmbeddingState>,
    workspace_id: String,
) -> Result<IndexStatus, FolioError> {
    let root = ai_boundary::resolve_workspace(state.inner(), &workspace_id)?;
    let index_state = index_state.inner().clone();
    let embedding_state = embedding_state.inner().clone();
    Ok(run_blocking::<_, FolioError, _>(move || {
        let snapshot = build_snapshot(&app, &embedding_state, &root)?;
        let status = snapshot_status(&snapshot);
        *index_state.lock().map_err(|_| NativeProviderError {
            code: folio_core::contracts::ProviderErrorCode::IoError,
            message: "The local index state is unavailable.".into(),
            detail: None,
        })? = Some(Arc::new(snapshot));
        Ok(status)
    })
    .await?)
}

fn snapshot_status(snapshot: &IndexSnapshot) -> IndexStatus {
    IndexStatus {
        workspace_id: Some(snapshot.workspace_id.clone()),
        document_count: snapshot.documents.len(),
        chunk_count: snapshot.chunks.len(),
        method: snapshot
            .embedding_space
            .as_ref()
            .map_or_else(|| "keyword".into(), |_| "hybrid".into()),
        space_fingerprint: snapshot
            .embedding_space
            .as_ref()
            .map(folio_core::retrieval::space_fingerprint),
        skipped_documents: snapshot.skipped_documents.clone(),
    }
}

fn annotate_embedding_space_failure(
    failure: CoreError,
    expected: &ProviderEmbeddingSpace,
    actual: &ProviderEmbeddingSpace,
) -> CoreError {
    match failure {
        CoreError::Provider(mut provider)
            if provider.code
                == folio_core::contracts::ProviderErrorCode::EmbeddingSpaceMismatch =>
        {
            provider.detail = Some(format!(
                "expected={};actual={}",
                folio_core::retrieval::space_fingerprint(expected),
                folio_core::retrieval::space_fingerprint(actual),
            ));
            CoreError::Provider(provider)
        }
        other => other,
    }
}

#[tauri::command]
fn index_status(index_state: State<'_, IndexState>) -> Result<IndexStatus, FolioError> {
    let guard = index_state.lock().map_err(|_| NativeProviderError {
        code: folio_core::contracts::ProviderErrorCode::IoError,
        message: "The local index state is unavailable.".into(),
        detail: None,
    })?;
    Ok(guard.as_ref().map_or(
        IndexStatus {
            workspace_id: None,
            document_count: 0,
            chunk_count: 0,
            method: "keyword".into(),
            space_fingerprint: None,
            skipped_documents: Vec::new(),
        },
        |snapshot| snapshot_status(snapshot),
    ))
}

fn ensure_snapshot(
    app: &AppHandle,
    embedding_state: &EmbeddingState,
    root: &ScopedRoot,
    index_state: &IndexState,
) -> Result<Arc<IndexSnapshot>, FolioError> {
    let current = corpus_fingerprint(root)?;
    if let Some(snapshot) = index_state
        .lock()
        .map_err(|_| NativeProviderError {
            code: folio_core::contracts::ProviderErrorCode::IoError,
            message: "The local index state is unavailable.".into(),
            detail: None,
        })?
        .as_ref()
        .filter(|snapshot| {
            snapshot.workspace_id == root.id && snapshot.source_fingerprint == current
        })
        .cloned()
    {
        return Ok(snapshot);
    }
    let snapshot = Arc::new(build_snapshot(app, embedding_state, root)?);
    *index_state.lock().map_err(|_| NativeProviderError {
        code: folio_core::contracts::ProviderErrorCode::IoError,
        message: "The local index state is unavailable.".into(),
        detail: None,
    })? = Some(snapshot.clone());
    Ok(snapshot)
}

#[tauri::command]
async fn semantic_search(
    app: AppHandle,
    state: State<'_, Folio>,
    index_state: State<'_, IndexState>,
    embedding_state: State<'_, EmbeddingState>,
    workspace_id: String,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<ProviderSearchResult>, FolioError> {
    let root = ai_boundary::resolve_workspace(state.inner(), &workspace_id)?;
    let index_state = index_state.inner().clone();
    let embedding_state = embedding_state.inner().clone();
    Ok(run_blocking::<_, FolioError, _>(move || {
        let snapshot = ensure_snapshot(&app, &embedding_state, &root, &index_state)?;
        let limit = limit.unwrap_or(10).clamp(1, 50);
        if snapshot.embedding_space.is_none() {
            return Ok(snapshot.retriever.keyword(
                &snapshot.documents,
                &snapshot.chunks,
                &query,
                limit,
            ));
        }
        let query_embedding = with_embedding_provider(&app, &embedding_state, |provider| {
            provider.embed_query(&query, None).map_err(native_error)
        })?
        .ok_or_else(|| NativeProviderError {
            code: folio_core::contracts::ProviderErrorCode::ModelNotInstalled,
            message: "The selected embedding model is no longer installed.".into(),
            detail: None,
        })?;
        Ok(snapshot
            .retriever
            .search(
                &snapshot.documents,
                &snapshot.chunks,
                &query,
                Some(&query_embedding),
                limit,
            )
            .map_err(|failure| {
                annotate_embedding_space_failure(
                    failure,
                    snapshot.embedding_space.as_ref().expect("semantic space"),
                    &query_embedding.space,
                )
            })?)
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

#[tauri::command]
fn cancel_generation(generation_state: State<'_, GenerationState>) -> Result<(), FolioError> {
    let guard = generation_state.lock().map_err(|_| NativeProviderError {
        code: folio_core::contracts::ProviderErrorCode::IoError,
        message: "The local generation state is unavailable.".into(),
        detail: None,
    })?;
    if let Some(cancel) = guard.active_cancel.as_ref() {
        cancel.store(true, Ordering::Release);
        if let Some(slot) = guard.slot.as_ref() {
            slot.provider.cancel_active().map_err(native_error)?;
        }
    }
    Ok(())
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

#[tauri::command]
async fn answer_question(
    app: AppHandle,
    state: State<'_, Folio>,
    index_state: State<'_, IndexState>,
    embedding_state: State<'_, EmbeddingState>,
    generation_state: State<'_, GenerationState>,
    workspace_id: String,
    question: String,
    document_id: Option<String>,
) -> Result<GroundedResult, FolioError> {
    let document_id = ai_boundary::validate_document_filter(&workspace_id, document_id.as_deref())?;
    let root = ai_boundary::resolve_workspace(state.inner(), &workspace_id)?;
    let index_state = index_state.inner().clone();
    let embedding_state = embedding_state.inner().clone();
    let generation_state = generation_state.inner().clone();
    Ok(run_blocking::<_, FolioError, _>(move || {
        let snapshot = ensure_snapshot(&app, &embedding_state, &root, &index_state)?;
        let results = search_snapshot(
            &app,
            &embedding_state,
            &snapshot,
            &question,
            document_id.as_deref(),
        )?;
        let passages = results
            .into_iter()
            .filter(|result| {
                document_id
                    .as_ref()
                    .is_none_or(|id| &result.document.id == id)
            })
            .flat_map(|result| result.passages)
            .take(folio_core::generation::MAX_PASSAGES)
            .collect::<Vec<_>>();
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

fn search_snapshot(
    app: &AppHandle,
    embedding_state: &EmbeddingState,
    snapshot: &IndexSnapshot,
    query: &str,
    document_id: Option<&str>,
) -> Result<Vec<ProviderSearchResult>, FolioError> {
    let limit = folio_core::generation::MAX_PASSAGES;
    if snapshot.embedding_space.is_none() {
        return Ok(snapshot
            .retriever
            .keyword(
                &snapshot.documents,
                &snapshot.chunks,
                query,
                snapshot.chunks.len(),
            )
            .into_iter()
            .filter(|result| document_id.is_none_or(|id| result.document.id == id))
            .take(limit)
            .collect());
    };
    let query_embedding = with_embedding_provider(app, embedding_state, |provider| {
        provider.embed_query(query, None).map_err(native_error)
    })?
    .ok_or_else(|| NativeProviderError {
        code: folio_core::contracts::ProviderErrorCode::ModelNotInstalled,
        message: "The selected embedding model is no longer installed.".into(),
        detail: None,
    })?;
    Ok(snapshot
        .retriever
        .search_scoped(
            &snapshot.documents,
            &snapshot.chunks,
            query,
            Some(&query_embedding),
            document_id,
            limit,
        )
        .map_err(|failure| {
            annotate_embedding_space_failure(
                failure,
                snapshot.embedding_space.as_ref().expect("semantic space"),
                &query_embedding.space,
            )
        })?)
}

#[tauri::command]
async fn interpret_request(
    app: AppHandle,
    state: State<'_, Folio>,
    generation_state: State<'_, GenerationState>,
    workspace_id: String,
    text: String,
) -> Result<InterpretationResult, FolioError> {
    let root = ai_boundary::resolve_workspace(state.inner(), &workspace_id)?;
    let generation_state = generation_state.inner().clone();
    Ok(run_blocking::<_, FolioError, _>(move || {
        let (documents, contents, chunks, _skipped_documents) = load_corpus(&root)?;
        let lease = acquire_generation(&app, &generation_state)?;
        let result = interpretation::interpret_request(
            lease.provider.as_ref(),
            &text,
            &documents,
            &contents,
            &chunks,
            lease.claim.cancel.as_ref(),
        );
        Ok(result?)
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
    loop {
        let mut guard = generation_state.lock().map_err(|_| NativeProviderError {
            code: folio_core::contracts::ProviderErrorCode::IoError,
            message: "The local generation state is unavailable.".into(),
            detail: None,
        })?;
        if let Some(cancel) = guard.active_cancel.as_ref() {
            cancel.store(true, Ordering::Release);
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
        let request = std::thread::spawn(move || {
            // Stand in for a request that notices cancellation and returns.
            while !claim.cancel.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(5));
            }
            std::thread::sleep(Duration::from_millis(40));
            drop(claim);
        });
        let generation_for_unload = generation.clone();
        let unloader = std::thread::spawn(move || unload_generation_now(&generation_for_unload));
        while !cancel.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(claim_free_slot(&generation).is_err());
        unloader.join().unwrap().unwrap();
        request.join().unwrap();
        assert!(generation.lock().unwrap().active_cancel.is_none());
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
    fn corpus_loading_skips_and_reports_unreadable_text() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("valid.md"), "valid content").unwrap();
        fs::write(root.path().join("invalid.md"), [0xff, 0xfe]).unwrap();

        let scoped_root = ScopedRoot {
            id: "test-workspace".into(),
            path: root.path().to_path_buf(),
        };
        let (documents, contents, chunks, skipped) = load_corpus(&scoped_root).unwrap();
        assert_eq!(documents.len(), 1);
        assert_eq!(contents.len(), 1);
        assert_eq!(chunks.len(), 1);
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].relative_path, "invalid.md");
        assert!(skipped[0].reason.contains("valid UTF-8"));
    }

    #[test]
    fn summary_record_preserves_the_native_document_id() {
        let root = ScopedRoot {
            id: "workspace".into(),
            path: PathBuf::from("/tmp/workspace"),
        };
        let text = DocumentText {
            content: "# Notes\nPaalala".into(),
            content_hash: "sha256:observed".into(),
            size_bytes: 15,
            modified_at_ms: Some(42),
            pages: Vec::new(),
            unreadable_pages: Vec::new(),
        };
        let record = document_record(
            &root,
            "workspace:notes/paalala.md",
            "notes/paalala.md",
            &text,
            &text.content,
        );
        assert_eq!(record.id, "workspace:notes/paalala.md");
        assert_eq!(record.relative_path, "notes/paalala.md");
        assert_eq!(record.content_hash.as_deref(), Some("sha256:observed"));
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
    fn every_managed_state_has_its_own_type() {
        use std::any::TypeId;
        // Tauri keeps one managed state per type, and a second `.manage` of the
        // same type panics before any window opens. Keep this list in step
        // with `run()` and its `setup`.
        let managed = [
            ("IndexState", TypeId::of::<IndexState>()),
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
        .manage(IndexState::default())
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
            register_embedding_space,
            pending_embedding_chunks,
            put_embeddings,
            vector_candidates,
            sync_embeddings,
            cancel_embedding_sync,
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

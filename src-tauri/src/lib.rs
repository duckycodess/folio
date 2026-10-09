mod config_guard;
mod contract_fixtures;
mod contracts;
mod db;
mod error;
mod extract;
mod identity;
mod index;
mod organize;
mod plan;
mod ripple;
mod workspace;
mod writer;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

use contracts::{ActionPlan, Approval, FileOperation, HistoryEntry, ImpactCandidate, UndoPreflight};
use error::{error, ErrorCode, FolioError};
use index::{
    ChunkVector, DuplicateGroup, EmbeddingSpace, ExplicitReference, IndexProgress,
    IndexedDocument, PendingChunk, ScanSummary, SearchResult, VectorCandidate,
};
use organize::OrganizationSuggestions;
use plan::PlanRegistry;
use writer::{ApplyReport, RealFileSystem, UndoReport};
use workspace::{
    DocumentListing, DocumentText, KnownWorkspace, WorkspaceInfo, WorkspaceRegistry,
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
) -> Result<Option<WorkspaceInfo>, FolioError> {
    let Some(folder) = app.dialog().file().blocking_pick_folder() else {
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
#[tauri::command]
async fn scan_workspace(
    app: AppHandle,
    state: State<'_, Folio>,
    workspace_id: String,
) -> Result<ScanSummary, FolioError> {
    let root = state.root(&workspace_id)?;
    let index_path = state.index_path.clone();
    let cancel = state.cancel_indexing.clone();
    let scanning = state.scanning.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _scanning = scanning.lock().map_err(|_| unavailable_state())?;
        cancel.store(false, Ordering::SeqCst);
        let mut conn = db::open(&index_path)?;
        index::scan_workspace(&mut conn, &root, &cancel, &mut |progress: &IndexProgress| {
            let _ = app.emit(INDEX_PROGRESS_EVENT, progress);
        })
    })
    .await
    .map_err(|cause| {
        error(ErrorCode::Internal, "Indexing stopped unexpectedly.").with_detail("cause", cause.to_string())
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
    operations: Vec<FileOperation>,
    impacts: Option<Vec<ImpactCandidate>>,
) -> Result<ActionPlan, FolioError> {
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
    let plan = plans.prepare(&workspace_id, operations, impacts, now, PLAN_LIFETIME_MS)?;
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let directory = app.path().app_data_dir()?;
            std::fs::create_dir_all(&directory)?;
            app.manage(Folio::open(directory.join("folio.sqlite"))?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            choose_workspace,
            list_workspaces,
            reopen_workspace,
            list_documents,
            read_document,
            scan_workspace,
            cancel_indexing,
            list_indexed_documents,
            search_index,
            list_duplicates,
            list_relationships,
            register_embedding_space,
            pending_embedding_chunks,
            put_embeddings,
            vector_candidates,
            prepare_plan,
            approve_plan,
            apply_plan,
            cancel_apply,
            preview_undo,
            undo_plan,
            list_history,
            ripple_impacts,
            prepare_passage_edit,
            organization_suggestions
        ])
        .run(tauri::generate_context!())
        .expect("Folio could not start");
}

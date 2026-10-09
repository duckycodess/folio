mod config_guard;
mod contract_fixtures;
mod contracts;
mod db;
mod error;
mod extract;
mod identity;
mod index;
mod plan;
mod workspace;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

use contracts::{ActionPlan, Approval, FileOperation, ImpactCandidate};
use error::{error, ErrorCode, FolioError};
use index::{
    ChunkVector, DuplicateGroup, EmbeddingSpace, ExplicitReference, IndexProgress,
    IndexedDocument, PendingChunk, ScanSummary, SearchResult, VectorCandidate,
};
use plan::PlanRegistry;
use workspace::{
    DocumentListing, DocumentText, KnownWorkspace, WorkspaceInfo, WorkspaceRegistry,
};

const INDEX_PROGRESS_EVENT: &str = "folio://index-progress";

/// How long a preview stays current. Approval and application both re-check it.
const PLAN_LIFETIME_MS: i64 = 5 * 60 * 1000;

struct Folio {
    workspaces: Mutex<WorkspaceRegistry>,
    plans: Mutex<PlanRegistry>,
    /// The persistent index in the OS application-data directory.
    index: Mutex<Connection>,
    index_path: PathBuf,
    /// One scan at a time; a second request waits and then finds little to do.
    scanning: Arc<Mutex<()>>,
    cancel_indexing: Arc<AtomicBool>,
}

impl Folio {
    fn open(index_path: PathBuf) -> Result<Self, FolioError> {
        Ok(Self {
            workspaces: Mutex::new(WorkspaceRegistry::new()),
            plans: Mutex::new(PlanRegistry::new()),
            index: Mutex::new(db::open(&index_path)?),
            index_path,
            scanning: Arc::new(Mutex::new(())),
            cancel_indexing: Arc::new(AtomicBool::new(false)),
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
    let mut workspaces = state.workspaces.lock().map_err(|_| unavailable_state())?;
    let info = workspaces.authorize(&path)?;
    workspace::remember(&*state.index()?, &info)?;
    Ok(Some(info))
}

/// Folders chosen in earlier sessions, with whether each is still reachable.
#[tauri::command]
async fn list_workspaces(state: State<'_, Folio>) -> Result<Vec<KnownWorkspace>, FolioError> {
    workspace::known_workspaces(&*state.index()?)
}

/// Restores a folder the user picked before. Access is revalidated and the
/// derived identity must still match; the webview never supplies a path.
#[tauri::command]
async fn reopen_workspace(
    state: State<'_, Folio>,
    workspace_id: String,
) -> Result<WorkspaceInfo, FolioError> {
    let path = workspace::remembered_root(&*state.index()?, &workspace_id)?;
    let mut workspaces = state.workspaces.lock().map_err(|_| unavailable_state())?;
    let info = workspaces.authorize(&path)?;
    if info.id != workspace_id {
        return Err(error(
            ErrorCode::WorkspaceUnavailable,
            "That folder now resolves to a different location. Choose it again.",
        ));
    }
    workspace::remember(&*state.index()?, &info)?;
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
    index::duplicate_groups(&*state.index()?, &root)
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
    let workspaces = state.workspaces.lock().map_err(|_| unavailable_state())?;
    let root = workspaces.resolve(&workspace_id)?;
    workspace::read_text(&root.path, &relative_path)
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
    let mut plans = state.plans.lock().map_err(|_| unavailable_state())?;
    let now = now_ms();
    let plan = plans.prepare(
        &workspace_id,
        operations,
        impacts.unwrap_or_default(),
        now,
        PLAN_LIFETIME_MS,
    )?;
    plan::preflight_plan(&root.path, &plan, now)?;
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
    plans.approve(&plan_id, &plan_digest, now_ms())
}

/// The native writer is issue #5. Until it exists this refuses, rather than
/// reporting a save the filesystem never made.
#[tauri::command]
async fn apply_plan(
    state: State<'_, Folio>,
    workspace_id: String,
    plan_id: String,
) -> Result<(), FolioError> {
    let workspaces = state.workspaces.lock().map_err(|_| unavailable_state())?;
    let root = workspaces.resolve(&workspace_id)?;
    let plans = state.plans.lock().map_err(|_| unavailable_state())?;
    plans.assert_can_apply(&root.path, &plan_id, now_ms())?;
    Err(plan::apply_not_implemented(&plan_id))
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
            apply_plan
        ])
        .run(tauri::generate_context!())
        .expect("Folio could not start");
}

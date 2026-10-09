mod db;
mod error;
mod extract;
mod index;
mod workspace;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use rusqlite::Connection;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use error::{fail, ErrorCode, NativeError, NativeResult};
use index::{ChunkVector, DuplicateGroup, EmbeddingSpace, IndexProgress, IndexedDocument, PendingChunk, Relationship, ScanSummary, SearchHit, VectorCandidate};
use workspace::{KnownWorkspace, ScopedRoot, WorkspaceInfo};

const INDEX_PROGRESS_EVENT: &str = "folio://index-progress";

struct AppState {
    db_path: PathBuf,
    conn: Mutex<Connection>,
    /// The folder authorized for this session; commands for any other workspace id are refused.
    active: Mutex<Option<ScopedRoot>>,
    cancel_indexing: Arc<AtomicBool>,
    indexing: AtomicBool,
}

impl AppState {
    fn conn(&self) -> NativeResult<MutexGuard<'_, Connection>> {
        self.conn.lock().map_err(|_| fail(ErrorCode::Database, "The index is unavailable."))
    }

    fn authorized(&self, workspace_id: &str) -> NativeResult<ScopedRoot> {
        self.active
            .lock()
            .map_err(|_| fail(ErrorCode::NotAuthorized, "Workspace state is unavailable."))?
            .as_ref()
            .filter(|root| root.id == workspace_id)
            .cloned()
            .ok_or_else(|| fail(ErrorCode::NotAuthorized, "Select an authorized folder first."))
    }

    fn activate(&self, root: ScopedRoot) -> NativeResult<WorkspaceInfo> {
        let info = root.info();
        *self.active.lock().map_err(|_| fail(ErrorCode::NotAuthorized, "Workspace state is unavailable."))? = Some(root);
        Ok(info)
    }
}

#[tauri::command]
async fn choose_workspace(app: AppHandle, state: State<'_, AppState>) -> NativeResult<Option<WorkspaceInfo>> {
    let Some(folder) = app.dialog().file().blocking_pick_folder() else { return Ok(None) };
    let picked = folder.into_path().map_err(|error| fail(ErrorCode::InvalidInput, error.to_string()))?;
    let root = workspace::remember_picked_folder(&*state.conn()?, &picked)?;
    state.activate(root).map(Some)
}

#[tauri::command]
async fn list_workspaces(state: State<'_, AppState>) -> NativeResult<Vec<KnownWorkspace>> {
    workspace::known_workspaces(&*state.conn()?)
}

#[tauri::command]
async fn reopen_workspace(state: State<'_, AppState>, workspace_id: String) -> NativeResult<WorkspaceInfo> {
    let root = workspace::reopen(&*state.conn()?, &workspace_id)?;
    state.activate(root)
}

#[tauri::command]
async fn list_documents(state: State<'_, AppState>, workspace_id: String) -> NativeResult<Vec<IndexedDocument>> {
    state.authorized(&workspace_id)?;
    index::list_documents(&*state.conn()?, &workspace_id)
}

#[tauri::command]
async fn read_document(state: State<'_, AppState>, workspace_id: String, relative_path: String) -> NativeResult<String> {
    let root = state.authorized(&workspace_id)?;
    workspace::read_text(&root.path, &relative_path)
}

#[tauri::command]
async fn scan_workspace(app: AppHandle, state: State<'_, AppState>, workspace_id: String) -> NativeResult<ScanSummary> {
    let root = state.authorized(&workspace_id)?;
    if state.indexing.swap(true, Ordering::SeqCst) {
        return Err(fail(ErrorCode::Busy, "Indexing is already running."));
    }
    state.cancel_indexing.store(false, Ordering::SeqCst);
    let db_path = state.db_path.clone();
    let cancel = state.cancel_indexing.clone();
    // A separate connection keeps search and reads responsive while indexing (WAL mode).
    let result = tauri::async_runtime::spawn_blocking(move || -> NativeResult<ScanSummary> {
        let mut conn = db::open(&db_path)?;
        index::scan_workspace(&mut conn, &root, &cancel, &mut |progress: &IndexProgress| {
            let _ = app.emit(INDEX_PROGRESS_EVENT, progress);
        })
    })
    .await
    .unwrap_or_else(|error| Err(fail(ErrorCode::Io, format!("Indexing stopped unexpectedly: {error}"))));
    state.indexing.store(false, Ordering::SeqCst);
    result
}

#[tauri::command]
fn cancel_indexing(state: State<'_, AppState>) {
    state.cancel_indexing.store(true, Ordering::SeqCst);
}

#[tauri::command]
async fn search_index(state: State<'_, AppState>, workspace_id: String, query: String, limit: Option<usize>) -> NativeResult<Vec<SearchHit>> {
    state.authorized(&workspace_id)?;
    index::search(&*state.conn()?, &workspace_id, &query, limit.unwrap_or(20))
}

#[tauri::command]
async fn list_duplicates(state: State<'_, AppState>, workspace_id: String) -> NativeResult<Vec<DuplicateGroup>> {
    let root = state.authorized(&workspace_id)?;
    index::duplicate_groups(&*state.conn()?, &root)
}

#[tauri::command]
async fn list_relationships(state: State<'_, AppState>, workspace_id: String) -> NativeResult<Vec<Relationship>> {
    state.authorized(&workspace_id)?;
    index::list_relationships(&*state.conn()?, &workspace_id)
}

#[tauri::command]
async fn register_embedding_space(state: State<'_, AppState>, space: EmbeddingSpace) -> NativeResult<String> {
    index::register_space(&*state.conn()?, &space)
}

#[tauri::command]
async fn pending_embedding_chunks(state: State<'_, AppState>, workspace_id: String, space_id: String, limit: Option<usize>) -> NativeResult<Vec<PendingChunk>> {
    state.authorized(&workspace_id)?;
    index::pending_embedding_chunks(&*state.conn()?, &workspace_id, &space_id, limit.unwrap_or(64))
}

#[tauri::command]
async fn put_embeddings(state: State<'_, AppState>, workspace_id: String, space_id: String, items: Vec<ChunkVector>) -> NativeResult<usize> {
    state.authorized(&workspace_id)?;
    index::put_embeddings(&mut *state.conn()?, &workspace_id, &space_id, &items)
}

#[tauri::command]
async fn vector_candidates(state: State<'_, AppState>, workspace_id: String, space_id: String, vector: Vec<f32>, k: Option<usize>) -> NativeResult<Vec<VectorCandidate>> {
    state.authorized(&workspace_id)?;
    index::vector_candidates(&*state.conn()?, &workspace_id, &space_id, &vector, k.unwrap_or(20))
}

fn open_state(app: &AppHandle) -> Result<AppState, NativeError> {
    let directory = app.path().app_data_dir().map_err(|error| fail(ErrorCode::Io, error.to_string()))?;
    std::fs::create_dir_all(&directory)?;
    let db_path = directory.join("folio.sqlite");
    let conn = db::open(&db_path)?;
    Ok(AppState { db_path, conn: Mutex::new(conn), active: Mutex::new(None), cancel_indexing: Arc::new(AtomicBool::new(false)), indexing: AtomicBool::new(false) })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let state = open_state(app.handle()).map_err(|error| error.to_string())?;
            app.manage(state);
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
            search_index,
            list_duplicates,
            list_relationships,
            register_embedding_space,
            pending_embedding_chunks,
            put_embeddings,
            vector_candidates
        ])
        .run(tauri::generate_context!())
        .expect("Folio could not start");
}

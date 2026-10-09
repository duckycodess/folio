mod workspace;

use std::sync::Mutex;
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use workspace::{DocumentMetadata, ScopedRoot};

type WorkspaceState = Mutex<Option<ScopedRoot>>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceInfo {
    id: String,
    root_path: String,
}

fn authorized(state: &State<'_, WorkspaceState>, workspace_id: &str) -> Result<ScopedRoot, String> {
    state.lock().map_err(|_| "Workspace state is unavailable.")?.as_ref().filter(|root| root.id == workspace_id).cloned().ok_or_else(|| "Select an authorized folder first.".into())
}

#[tauri::command]
async fn choose_workspace(app: AppHandle, state: State<'_, WorkspaceState>) -> Result<Option<WorkspaceInfo>, String> {
    let folder = app.dialog().file().blocking_pick_folder();
    let Some(folder) = folder else { return Ok(None); };
    let path = folder.into_path().map_err(|error| error.to_string())?.canonicalize().map_err(|error| error.to_string())?;
    if !path.is_dir() { return Err("Choose a directory.".into()); }
    let id = uuid::Uuid::new_v4().to_string();
    let info = WorkspaceInfo { id: id.clone(), root_path: path.to_string_lossy().into_owned() };
    *state.lock().map_err(|_| "Workspace state is unavailable.")? = Some(ScopedRoot { id, path });
    Ok(Some(info))
}

#[tauri::command]
async fn list_documents(state: State<'_, WorkspaceState>, workspace_id: String) -> Result<Vec<DocumentMetadata>, String> {
    let root = authorized(&state, &workspace_id)?;
    workspace::list_documents(&root.path)
}

#[tauri::command]
async fn read_document(state: State<'_, WorkspaceState>, workspace_id: String, relative_path: String) -> Result<String, String> {
    let root = authorized(&state, &workspace_id)?;
    workspace::read_text(&root.path, &relative_path)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(WorkspaceState::new(None))
        .invoke_handler(tauri::generate_handler![choose_workspace, list_documents, read_document])
        .run(tauri::generate_context!())
        .expect("Folio could not start");
}

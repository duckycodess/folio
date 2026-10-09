mod config_guard;
mod contract_fixtures;
mod contracts;
mod error;
mod identity;
mod plan;
mod workspace;

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use contracts::{ActionPlan, Approval, FileOperation, ImpactCandidate};
use error::{error, ErrorCode, FolioError};
use plan::PlanRegistry;
use workspace::{DocumentListing, DocumentText, WorkspaceInfo, WorkspaceRegistry};

/// How long a preview stays current. Approval and application both re-check it.
const PLAN_LIFETIME_MS: i64 = 5 * 60 * 1000;

struct Folio {
    workspaces: Mutex<WorkspaceRegistry>,
    plans: Mutex<PlanRegistry>,
}

impl Folio {
    fn new() -> Self {
        Self {
            workspaces: Mutex::new(WorkspaceRegistry::new()),
            plans: Mutex::new(PlanRegistry::new()),
        }
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
    Ok(Some(workspaces.authorize(&path)?))
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
        .manage(Folio::new())
        .invoke_handler(tauri::generate_handler![
            choose_workspace,
            list_documents,
            read_document,
            prepare_plan,
            approve_plan,
            apply_plan
        ])
        .run(tauri::generate_context!())
        .expect("Folio could not start");
}

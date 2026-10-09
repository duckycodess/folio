mod ai_boundary;
mod config_guard;
mod contract_fixtures;
mod contracts;
mod error;
mod identity;
mod plan;
mod workspace;

use contracts::{ActionPlan, Approval, FileOperation, ImpactCandidate};
use error::{error, ErrorCode, FolioError};
use folio_core::chunking::{Chunk, ChunkSource, InterimTextChunker, TextDocument};
use folio_core::contracts::{
    DocumentRecord, EmbeddingSpace, GroundedAnswer, InterpretationResult, Language,
    ModelDescriptor, ModelInstallState, ModelRole, NativeProviderError, SearchResult,
};
use folio_core::embeddings::{EmbeddingKind, EmbeddingProvider, OrtE5Provider};
use folio_core::error::CoreError;
use folio_core::generation::{GenerationProvider, LlamaServerProvider};
use folio_core::grounding;
use folio_core::interpretation;
use folio_core::models::{DownloadProgress, ModelStore, RuntimeStatus};
use folio_core::retrieval::HybridRetriever;
use identity::media_type_for_path;
use plan::PlanRegistry;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use workspace::{DocumentListing, DocumentText, ScopedRoot, WorkspaceInfo, WorkspaceRegistry};

/// The issue #2 native boundary owns workspace identities and plan authority.
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

const PLAN_LIFETIME_MS: i64 = 5 * 60 * 1000;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis() as i64)
        .unwrap_or_default()
}

fn unavailable_state() -> FolioError {
    error(
        ErrorCode::Internal,
        "Folio's workspace or plan state is unavailable.",
    )
}

#[derive(Clone)]
struct IndexSnapshot {
    workspace_id: String,
    documents: Vec<DocumentRecord>,
    chunks: Vec<Chunk>,
    retriever: HybridRetriever,
    embedding_space: Option<EmbeddingSpace>,
    skipped_documents: Vec<SkippedDocument>,
}

type IndexState = Arc<Mutex<Option<IndexSnapshot>>>;

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

async fn run_blocking<T, F>(work: F) -> Result<T, NativeProviderError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, NativeProviderError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| NativeProviderError {
            code: folio_core::contracts::ProviderErrorCode::IoError,
            message: "The native operation stopped unexpectedly.".into(),
            detail: Some(error.to_string()),
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
async fn choose_workspace(
    app: AppHandle,
    state: State<'_, Folio>,
    index_state: State<'_, IndexState>,
) -> Result<Option<WorkspaceInfo>, FolioError> {
    let Some(folder) = app.dialog().file().blocking_pick_folder() else {
        return Ok(None);
    };
    let path = folder
        .into_path()
        .map_err(|cause| error(ErrorCode::WorkspaceUnavailable, cause.to_string()))?;
    let info = {
        let mut workspaces = state.workspaces.lock().map_err(|_| unavailable_state())?;
        workspaces.authorize(&path)?
    };
    *index_state.lock().map_err(|_| unavailable_state())? = None;
    Ok(Some(info))
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

/// Prepare an exact proposal. This is read-only; the issue #5 writer is not
/// present on the #4 path and no AI result can bypass this plan boundary.
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

/// Approval binds to the native-issued plan identity and digest.
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

/// No file write is claimed here. The native writer remains issue #5.
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

#[tauri::command]
async fn list_models(app: AppHandle) -> Result<Vec<ModelDescriptor>, NativeProviderError> {
    run_blocking(move || Ok(model_store(&app)?.manifest().models.clone())).await
}

#[tauri::command]
async fn verify_model(
    app: AppHandle,
    model_id: String,
) -> Result<ModelInstallState, NativeProviderError> {
    run_blocking(move || {
        model_store(&app)?
            .verify_model(&model_id)
            .map_err(native_error)
    })
    .await
}

#[tauri::command]
async fn install_model(
    app: AppHandle,
    index_state: State<'_, IndexState>,
    embedding_state: State<'_, EmbeddingState>,
    install_state: State<'_, InstallState>,
    model_id: String,
) -> Result<ModelInstallState, NativeProviderError> {
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
    result
}

#[tauri::command]
async fn remove_model(
    app: AppHandle,
    index_state: State<'_, IndexState>,
    embedding_state: State<'_, EmbeddingState>,
    model_id: String,
) -> Result<(), NativeProviderError> {
    let index_state = index_state.inner().clone();
    let embedding_state = embedding_state.inner().clone();
    let result = run_blocking(move || {
        let result = model_store(&app)?
            .remove_model(&model_id)
            .map_err(native_error);
        unload_embedding(&embedding_state)?;
        result
    })
    .await;
    invalidate_index(&index_state)?;
    result
}

#[tauri::command]
async fn select_model(
    app: AppHandle,
    index_state: State<'_, IndexState>,
    embedding_state: State<'_, EmbeddingState>,
    role: ModelRole,
    model_id: String,
) -> Result<(), NativeProviderError> {
    let embedding_selection = matches!(&role, ModelRole::Embedding);
    let index_state = index_state.inner().clone();
    let embedding_state = embedding_state.inner().clone();
    let result = run_blocking(move || {
        let result = model_store(&app)?
            .select_model(role, &model_id)
            .map_err(native_error);
        if embedding_selection {
            unload_embedding(&embedding_state)?;
        }
        result
    })
    .await;
    if result.is_ok() && embedding_selection {
        invalidate_index(&index_state)?;
    }
    result
}

#[tauri::command]
async fn runtime_status(
    app: AppHandle,
    runtime_id: String,
) -> Result<RuntimeStatus, NativeProviderError> {
    run_blocking(move || {
        model_store(&app)?
            .runtime_status(&runtime_id)
            .map_err(native_error)
    })
    .await
}

#[tauri::command]
async fn install_runtime(
    app: AppHandle,
    runtime_id: String,
    install_state: State<'_, InstallState>,
) -> Result<RuntimeStatus, NativeProviderError> {
    let cancel = begin_install(install_state.inner())?;
    let worker_cancel = cancel.clone();
    let install_state = install_state.inner().clone();
    let progress_app = app.clone();
    let result = run_blocking(move || {
        model_store(&app)?
            .install_runtime(&runtime_id, &worker_cancel, |progress: DownloadProgress| {
                let _ = progress_app.emit("folio://runtime-progress", progress);
            })
            .map_err(native_error)
    })
    .await;
    finish_install(&install_state, &cancel)?;
    result
}

#[tauri::command]
fn cancel_install(install_state: State<'_, InstallState>) -> Result<(), NativeProviderError> {
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
    String,
> {
    let metadata = workspace::list_documents(root)
        .map_err(|error| error.to_string())?
        .documents;
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
            title: markdown_title(&row.name, &content),
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
    let chunks = InterimTextChunker::new(text_documents)
        .all_chunks()
        .map_err(|error| error.to_string())?;
    Ok((documents, contents, chunks, skipped_documents))
}

fn markdown_title(name: &str, content: &str) -> String {
    content
        .lines()
        .find_map(|line| {
            line.strip_prefix("# ")
                .map(str::trim)
                .filter(|title| !title.is_empty())
        })
        .unwrap_or(name)
        .to_owned()
}

fn with_embedding_provider<T, F>(
    app: &AppHandle,
    embedding_state: &EmbeddingState,
    work: F,
) -> Result<Option<T>, NativeProviderError>
where
    F: FnOnce(&OrtE5Provider) -> Result<T, NativeProviderError>,
{
    let store = model_store(app)?;
    let mut guard = embedding_state.lock().map_err(|_| NativeProviderError {
        code: folio_core::contracts::ProviderErrorCode::IoError,
        message: "The local embedding state is unavailable.".into(),
        detail: None,
    })?;
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
    if guard
        .as_ref()
        .is_none_or(|slot| slot.model_id != descriptor.id || slot.revision != descriptor.revision)
    {
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
    guard
        .as_ref()
        .ok_or_else(|| NativeProviderError {
            code: folio_core::contracts::ProviderErrorCode::IoError,
            message: "The local embedding provider is unavailable.".into(),
            detail: None,
        })
        .and_then(|slot| work(&slot.provider))
        .map(Some)
}

fn build_snapshot(
    app: &AppHandle,
    embedding_state: &EmbeddingState,
    root: &ScopedRoot,
) -> Result<IndexSnapshot, NativeProviderError> {
    let (documents, _contents, chunks, skipped_documents) =
        load_corpus(root).map_err(|error| NativeProviderError {
            code: folio_core::contracts::ProviderErrorCode::IoError,
            message: error,
            detail: None,
        })?;
    let mut retriever = HybridRetriever::default();
    let mut embedding_space = None;
    if let Some((space, vectors)) = with_embedding_provider(app, embedding_state, |provider| {
        let texts = chunks
            .iter()
            .map(|chunk| chunk.text.clone())
            .collect::<Vec<_>>();
        let vectors = provider
            .embed(&texts, EmbeddingKind::Passage, None)
            .map_err(native_error)?;
        Ok((provider.space().clone(), vectors))
    })? {
        retriever
            .vector_index
            .replace(space.clone(), chunks.clone(), vectors)
            .map_err(native_error)?;
        embedding_space = Some(space);
    }
    Ok(IndexSnapshot {
        workspace_id: root.id.clone(),
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
) -> Result<IndexStatus, NativeProviderError> {
    let root = ai_boundary::resolve_workspace(state.inner(), &workspace_id).map_err(|error| {
        NativeProviderError {
            code: folio_core::contracts::ProviderErrorCode::IoError,
            message: error.message,
            detail: None,
        }
    })?;
    let index_state = index_state.inner().clone();
    let embedding_state = embedding_state.inner().clone();
    run_blocking(move || {
        let snapshot = build_snapshot(&app, &embedding_state, &root)?;
        let status = snapshot_status(&snapshot);
        *index_state.lock().map_err(|_| NativeProviderError {
            code: folio_core::contracts::ProviderErrorCode::IoError,
            message: "The local index state is unavailable.".into(),
            detail: None,
        })? = Some(snapshot);
        Ok(status)
    })
    .await
}

fn snapshot_status(snapshot: &IndexSnapshot) -> IndexStatus {
    IndexStatus {
        workspace_id: Some(snapshot.workspace_id.clone()),
        document_count: snapshot.documents.len(),
        chunk_count: snapshot.chunks.len(),
        method: snapshot
            .embedding_space
            .as_ref()
            .map_or_else(|| "keyword".into(), |_| "semantic".into()),
        space_fingerprint: snapshot
            .embedding_space
            .as_ref()
            .map(folio_core::retrieval::space_fingerprint),
        skipped_documents: snapshot.skipped_documents.clone(),
    }
}

#[tauri::command]
fn index_status(index_state: State<'_, IndexState>) -> Result<IndexStatus, NativeProviderError> {
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
        snapshot_status,
    ))
}

fn ensure_snapshot(
    app: &AppHandle,
    embedding_state: &EmbeddingState,
    root: &ScopedRoot,
    index_state: &IndexState,
) -> Result<IndexSnapshot, NativeProviderError> {
    if let Some(snapshot) = index_state
        .lock()
        .map_err(|_| NativeProviderError {
            code: folio_core::contracts::ProviderErrorCode::IoError,
            message: "The local index state is unavailable.".into(),
            detail: None,
        })?
        .as_ref()
        .filter(|snapshot| snapshot.workspace_id == root.id)
        .cloned()
    {
        return Ok(snapshot);
    }
    let snapshot = build_snapshot(app, embedding_state, root)?;
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
) -> Result<Vec<SearchResult>, NativeProviderError> {
    let root = ai_boundary::resolve_workspace(state.inner(), &workspace_id).map_err(|error| {
        NativeProviderError {
            code: folio_core::contracts::ProviderErrorCode::IoError,
            message: error.message,
            detail: None,
        }
    })?;
    let index_state = index_state.inner().clone();
    let embedding_state = embedding_state.inner().clone();
    run_blocking(move || {
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
        snapshot
            .retriever
            .search(
                &snapshot.documents,
                &snapshot.chunks,
                &query,
                Some(&query_embedding),
                limit,
            )
            .map_err(native_error)
    })
    .await
}

fn generation_provider(
    app: &AppHandle,
    generation_state: &GenerationState,
) -> Result<Arc<LlamaServerProvider>, NativeProviderError> {
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
    let runtime = store
        .runtime_status(runtime_id_for_host())
        .map_err(native_error)?;
    let executable = runtime.executable_path.ok_or_else(|| NativeProviderError {
        code: folio_core::contracts::ProviderErrorCode::RuntimeMissing,
        message: "Install the pinned llama.cpp runtime for this platform first.".into(),
        detail: Some(runtime_id_for_host().into()),
    })?;
    let mut guard = generation_state.lock().map_err(|_| NativeProviderError {
        code: folio_core::contracts::ProviderErrorCode::IoError,
        message: "The local generation state is unavailable.".into(),
        detail: None,
    })?;
    if guard.active_cancel.is_some() {
        return Err(NativeProviderError {
            code: folio_core::contracts::ProviderErrorCode::GenerationBusy,
            message: "Another local generation request is active.".into(),
            detail: None,
        });
    }
    if let Some(slot) = guard.slot.as_ref() {
        if slot.model_id == verified.descriptor.id && slot.revision == verified.descriptor.revision
        {
            return Ok(slot.provider.clone());
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
    Ok(provider)
}

fn begin_generation(
    generation_state: &GenerationState,
) -> Result<Arc<AtomicBool>, NativeProviderError> {
    let mut guard = generation_state.lock().map_err(|_| NativeProviderError {
        code: folio_core::contracts::ProviderErrorCode::IoError,
        message: "The local generation state is unavailable.".into(),
        detail: None,
    })?;
    if guard.active_cancel.is_some() {
        return Err(NativeProviderError {
            code: folio_core::contracts::ProviderErrorCode::GenerationBusy,
            message: "Another local generation request is active.".into(),
            detail: None,
        });
    }
    let cancel = Arc::new(AtomicBool::new(false));
    guard.active_cancel = Some(cancel.clone());
    Ok(cancel)
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
fn cancel_generation(
    generation_state: State<'_, GenerationState>,
) -> Result<(), NativeProviderError> {
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
) -> Result<GroundedAnswer, NativeProviderError> {
    let root = ai_boundary::resolve_workspace(state.inner(), &workspace_id).map_err(|error| {
        NativeProviderError {
            code: folio_core::contracts::ProviderErrorCode::IoError,
            message: error.message,
            detail: None,
        }
    })?;
    let generation_state = generation_state.inner().clone();
    run_blocking(move || {
        let document_text = workspace::read_text(&root.path, &document_id).map_err(|error| {
            NativeProviderError {
                code: folio_core::contracts::ProviderErrorCode::IoError,
                message: error.to_string(),
                detail: None,
            }
        })?;
        let content = document_text.content.clone();
        let record = DocumentRecord {
            id: document_id.clone(),
            workspace_id: workspace_id.clone(),
            relative_path: document_id.clone(),
            name: Path::new(&document_id)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(&document_id)
                .into(),
            title: markdown_title(&document_id, &content),
            language: grounding::detect_language(&content),
            media_type: media_type_for_path(&document_id)
                .unwrap_or("text/plain")
                .into(),
            size_bytes: document_text.size_bytes,
            modified_at_ms: document_text.modified_at_ms,
            content: Some(content.clone()),
            content_hash: Some(document_text.content_hash),
        };
        let chunks = InterimTextChunker::new(vec![TextDocument::new(record, content.clone())])
            .chunks(&document_id)
            .map_err(native_error)?;
        let provider = generation_provider(&app, &generation_state)?;
        let cancel = begin_generation(&generation_state)?;
        let result = grounding::summarize_document(
            provider.as_ref(),
            grounding::passages_from_chunks(&chunks),
            grounding::detect_language(&content),
            cancel.as_ref(),
        );
        finish_generation(&generation_state, &cancel)?;
        result.map_err(native_error)
    })
    .await
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
) -> Result<GroundedAnswer, NativeProviderError> {
    let root = ai_boundary::resolve_workspace(state.inner(), &workspace_id).map_err(|error| {
        NativeProviderError {
            code: folio_core::contracts::ProviderErrorCode::IoError,
            message: error.message,
            detail: None,
        }
    })?;
    let index_state = index_state.inner().clone();
    let embedding_state = embedding_state.inner().clone();
    let generation_state = generation_state.inner().clone();
    run_blocking(move || {
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
            return grounding::answer_question(
                None,
                &question,
                passages,
                grounding::detect_language(&question),
                &AtomicBool::new(false),
            )
            .map_err(native_error);
        }
        let provider = generation_provider(&app, &generation_state)?;
        let cancel = begin_generation(&generation_state)?;
        let result = grounding::answer_question(
            Some(provider.as_ref()),
            &question,
            passages,
            grounding::detect_language(&question),
            cancel.as_ref(),
        );
        finish_generation(&generation_state, &cancel)?;
        result.map_err(native_error)
    })
    .await
}

fn search_snapshot(
    app: &AppHandle,
    embedding_state: &EmbeddingState,
    snapshot: &IndexSnapshot,
    query: &str,
    document_id: Option<&str>,
) -> Result<Vec<SearchResult>, NativeProviderError> {
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
    snapshot
        .retriever
        .search_scoped(
            &snapshot.documents,
            &snapshot.chunks,
            query,
            Some(&query_embedding),
            document_id,
            limit,
        )
        .map_err(native_error)
}

#[tauri::command]
async fn interpret_request(
    app: AppHandle,
    state: State<'_, Folio>,
    generation_state: State<'_, GenerationState>,
    workspace_id: String,
    text: String,
) -> Result<InterpretationResult, NativeProviderError> {
    let root = ai_boundary::resolve_workspace(state.inner(), &workspace_id).map_err(|error| {
        NativeProviderError {
            code: folio_core::contracts::ProviderErrorCode::IoError,
            message: error.message,
            detail: None,
        }
    })?;
    let generation_state = generation_state.inner().clone();
    run_blocking(move || {
        let (documents, contents, chunks, _skipped_documents) =
            load_corpus(&root).map_err(|message| NativeProviderError {
                code: folio_core::contracts::ProviderErrorCode::IoError,
                message,
                detail: None,
            })?;
        let provider = generation_provider(&app, &generation_state)?;
        let cancel = begin_generation(&generation_state)?;
        let result = interpretation::interpret_request(
            provider.as_ref(),
            &text,
            &documents,
            &contents,
            &chunks,
            cancel.as_ref(),
        );
        finish_generation(&generation_state, &cancel)?;
        result.map_err(native_error)
    })
    .await
}

fn unload_generation_now(generation_state: &GenerationState) -> Result<(), NativeProviderError> {
    let mut guard = generation_state.lock().map_err(|_| NativeProviderError {
        code: folio_core::contracts::ProviderErrorCode::IoError,
        message: "The local generation state is unavailable.".into(),
        detail: None,
    })?;
    if let Some(cancel) = guard.active_cancel.take() {
        cancel.store(true, Ordering::Release);
    }
    if let Some(slot) = guard.slot.take() {
        slot.provider.unload().map_err(native_error)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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
}

#[tauri::command]
async fn unload_generation(
    generation_state: State<'_, GenerationState>,
) -> Result<(), NativeProviderError> {
    let generation_state = generation_state.inner().clone();
    run_blocking(move || unload_generation_now(&generation_state)).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Folio::new())
        .manage(Arc::new(Mutex::new(None::<IndexSnapshot>)))
        .manage(Arc::new(Mutex::new(None::<EmbeddingSlot>)))
        .manage(Arc::new(Mutex::new(GenerationStateInner::default())))
        .manage(Arc::new(Mutex::new(None::<Arc<AtomicBool>>)))
        .invoke_handler(tauri::generate_handler![
            choose_workspace,
            list_documents,
            read_document,
            prepare_plan,
            approve_plan,
            apply_plan,
            list_models,
            verify_model,
            install_model,
            remove_model,
            select_model,
            runtime_status,
            install_runtime,
            cancel_install,
            rebuild_index,
            index_status,
            semantic_search,
            summarize_document,
            answer_question,
            interpret_request,
            cancel_generation,
            unload_generation
        ])
        .build(tauri::generate_context!())
        .expect("Folio could not start");
    app.run(|app_handle, event| {
        if matches!(
            event,
            tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
        ) {
            if let Some(generation_state) = app_handle.try_state::<GenerationState>() {
                let _ = unload_generation_now(generation_state.inner());
            }
        }
    });
}

mod workspace;

use folio_core::chunking::{sha256, Chunk, ChunkSource, InterimTextChunker, TextDocument};
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
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use workspace::{DocumentMetadata, ScopedRoot};

type WorkspaceState = Mutex<Option<ScopedRoot>>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceInfo {
    id: String,
    root_path: String,
}

#[derive(Clone)]
struct IndexSnapshot {
    workspace_id: String,
    documents: Vec<DocumentRecord>,
    chunks: Vec<Chunk>,
    retriever: HybridRetriever,
    embedding_space: Option<EmbeddingSpace>,
}

type IndexState = Mutex<Option<IndexSnapshot>>;

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

type GenerationState = Mutex<GenerationStateInner>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IndexStatus {
    workspace_id: Option<String>,
    document_count: usize,
    chunk_count: usize,
    method: String,
    embedding_space_id: Option<String>,
}

fn authorized(state: &State<'_, WorkspaceState>, workspace_id: &str) -> Result<ScopedRoot, String> {
    state
        .lock()
        .map_err(|_| "Workspace state is unavailable.")?
        .as_ref()
        .filter(|root| root.id == workspace_id)
        .cloned()
        .ok_or_else(|| "Select an authorized folder first.".into())
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

#[tauri::command]
async fn choose_workspace(
    app: AppHandle,
    state: State<'_, WorkspaceState>,
) -> Result<Option<WorkspaceInfo>, String> {
    let folder = app.dialog().file().blocking_pick_folder();
    let Some(folder) = folder else {
        return Ok(None);
    };
    let path = folder
        .into_path()
        .map_err(|error| error.to_string())?
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !path.is_dir() {
        return Err("Choose a directory.".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let info = WorkspaceInfo {
        id: id.clone(),
        root_path: path.to_string_lossy().into_owned(),
    };
    *state
        .lock()
        .map_err(|_| "Workspace state is unavailable.")? = Some(ScopedRoot { id, path });
    Ok(Some(info))
}

#[tauri::command]
async fn list_documents(
    state: State<'_, WorkspaceState>,
    workspace_id: String,
) -> Result<Vec<DocumentMetadata>, String> {
    let root = authorized(&state, &workspace_id)?;
    workspace::list_documents(&root.path)
}

#[tauri::command]
async fn read_document(
    state: State<'_, WorkspaceState>,
    workspace_id: String,
    relative_path: String,
) -> Result<String, String> {
    let root = authorized(&state, &workspace_id)?;
    workspace::read_text(&root.path, &relative_path)
}

#[tauri::command]
fn list_models(app: AppHandle) -> Result<Vec<ModelDescriptor>, NativeProviderError> {
    Ok(model_store(&app)?.manifest().models.clone())
}

#[tauri::command]
fn verify_model(
    app: AppHandle,
    model_id: String,
) -> Result<ModelInstallState, NativeProviderError> {
    model_store(&app)?
        .verify_model(&model_id)
        .map_err(native_error)
}

#[tauri::command]
fn install_model(
    app: AppHandle,
    model_id: String,
) -> Result<ModelInstallState, NativeProviderError> {
    let store = model_store(&app)?;
    let cancel = AtomicBool::new(false);
    store
        .install_model(&model_id, &cancel, |progress| {
            let _ = app.emit("folio://model-progress", progress);
        })
        .map_err(native_error)
}

#[tauri::command]
fn remove_model(app: AppHandle, model_id: String) -> Result<(), NativeProviderError> {
    model_store(&app)?
        .remove_model(&model_id)
        .map_err(native_error)
}

#[tauri::command]
fn select_model(
    app: AppHandle,
    role: ModelRole,
    model_id: String,
) -> Result<(), NativeProviderError> {
    model_store(&app)?
        .select_model(role, &model_id)
        .map_err(native_error)
}

#[tauri::command]
fn runtime_status(
    app: AppHandle,
    runtime_id: String,
) -> Result<RuntimeStatus, NativeProviderError> {
    model_store(&app)?
        .runtime_status(&runtime_id)
        .map_err(native_error)
}

#[tauri::command]
fn install_runtime(
    app: AppHandle,
    runtime_id: String,
) -> Result<RuntimeStatus, NativeProviderError> {
    let store = model_store(&app)?;
    let cancel = AtomicBool::new(false);
    store
        .install_runtime(&runtime_id, &cancel, |progress: DownloadProgress| {
            let _ = app.emit("folio://runtime-progress", progress);
        })
        .map_err(native_error)
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
    root: &Path,
) -> Result<(Vec<DocumentRecord>, HashMap<String, String>, Vec<Chunk>), String> {
    let metadata = workspace::list_documents(root)?;
    let mut documents = Vec::new();
    let mut contents = HashMap::new();
    let mut text_documents = Vec::new();
    for row in metadata {
        let extension = Path::new(&row.relative_path)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !matches!(extension.as_str(), "txt" | "md") {
            continue;
        }
        let content = workspace::read_text(root, &row.relative_path)?;
        let record = DocumentRecord {
            id: row.id.clone(),
            relative_path: row.relative_path.clone(),
            name: row.name,
            title: row.id.clone(),
            language: Language::Unknown,
            size_bytes: row.size_bytes,
            content: Some(content.clone()),
            content_hash: Some(sha256(&content)),
        };
        contents.insert(record.id.clone(), content.clone());
        text_documents.push(TextDocument::new(record.clone(), content));
        documents.push(record);
    }
    let chunks = InterimTextChunker::new(text_documents)
        .all_chunks()
        .map_err(|error| error.to_string())?;
    Ok((documents, contents, chunks))
}

fn optional_embedding_provider(
    store: &ModelStore,
) -> Result<Option<OrtE5Provider>, NativeProviderError> {
    let Some(model_id) = store
        .selected_model(ModelRole::Embedding)
        .map_err(native_error)?
    else {
        return Ok(None);
    };
    let descriptor = store.model(&model_id).map_err(native_error)?.clone();
    let state = store.verify_model(&model_id).map_err(native_error)?;
    if !matches!(
        state.status,
        folio_core::contracts::ModelInstallStatus::Installed
    ) {
        return Ok(None);
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
    let model_id_value = descriptor.id.clone();
    let revision = descriptor.revision.clone();
    let quantization = descriptor.quantization.clone();
    let model_sha256 = model_file.sha256.clone();
    let tokenizer_sha256 = tokenizer_file.sha256.clone();
    OrtE5Provider::from_files(
        model_path,
        tokenizer_path,
        model_id_value,
        revision,
        quantization,
        384,
        &model_sha256,
        &tokenizer_sha256,
        folio_core::embeddings::DEFAULT_MAX_TOKENS,
        folio_core::embeddings::DEFAULT_BATCH_SIZE,
        2,
    )
    .map(Some)
    .map_err(native_error)
}

fn build_snapshot(
    app: &AppHandle,
    workspace_id: &str,
    root: &Path,
) -> Result<IndexSnapshot, NativeProviderError> {
    let (documents, _contents, chunks) =
        load_corpus(root).map_err(|error| NativeProviderError {
            code: folio_core::contracts::ProviderErrorCode::IoError,
            message: error,
            detail: None,
        })?;
    let store = model_store(app)?;
    let mut retriever = HybridRetriever::default();
    let mut embedding_space = None;
    if let Some(provider) = optional_embedding_provider(&store)? {
        let texts = chunks
            .iter()
            .map(|chunk| chunk.text.clone())
            .collect::<Vec<_>>();
        let vectors = provider
            .embed(&texts, EmbeddingKind::Passage, None)
            .map_err(native_error)?;
        let space = provider.space().clone();
        retriever
            .vector_index
            .replace(space.clone(), chunks.clone(), vectors)
            .map_err(native_error)?;
        provider.unload().map_err(native_error)?;
        embedding_space = Some(space);
    }
    Ok(IndexSnapshot {
        workspace_id: workspace_id.into(),
        documents,
        chunks,
        retriever,
        embedding_space,
    })
}

#[tauri::command]
fn rebuild_index(
    app: AppHandle,
    state: State<'_, WorkspaceState>,
    index_state: State<'_, IndexState>,
    workspace_id: String,
) -> Result<IndexStatus, NativeProviderError> {
    let root = authorized(&state, &workspace_id).map_err(|message| NativeProviderError {
        code: folio_core::contracts::ProviderErrorCode::IoError,
        message,
        detail: None,
    })?;
    let snapshot = build_snapshot(&app, &workspace_id, &root.path)?;
    let status = snapshot_status(&snapshot);
    *index_state.lock().map_err(|_| NativeProviderError {
        code: folio_core::contracts::ProviderErrorCode::IoError,
        message: "The local index state is unavailable.".into(),
        detail: None,
    })? = Some(snapshot);
    Ok(status)
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
        embedding_space_id: snapshot
            .embedding_space
            .as_ref()
            .map(folio_core::retrieval::embedding_space_id),
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
            embedding_space_id: None,
        },
        snapshot_status,
    ))
}

fn ensure_snapshot(
    app: &AppHandle,
    state: &State<'_, WorkspaceState>,
    index_state: &State<'_, IndexState>,
    workspace_id: &str,
) -> Result<IndexSnapshot, NativeProviderError> {
    if let Some(snapshot) = index_state
        .lock()
        .map_err(|_| NativeProviderError {
            code: folio_core::contracts::ProviderErrorCode::IoError,
            message: "The local index state is unavailable.".into(),
            detail: None,
        })?
        .as_ref()
        .filter(|snapshot| snapshot.workspace_id == workspace_id)
        .cloned()
    {
        return Ok(snapshot);
    }
    let root = authorized(state, workspace_id).map_err(|message| NativeProviderError {
        code: folio_core::contracts::ProviderErrorCode::IoError,
        message,
        detail: None,
    })?;
    let snapshot = build_snapshot(app, workspace_id, &root.path)?;
    *index_state.lock().map_err(|_| NativeProviderError {
        code: folio_core::contracts::ProviderErrorCode::IoError,
        message: "The local index state is unavailable.".into(),
        detail: None,
    })? = Some(snapshot.clone());
    Ok(snapshot)
}

#[tauri::command]
fn semantic_search(
    app: AppHandle,
    state: State<'_, WorkspaceState>,
    index_state: State<'_, IndexState>,
    workspace_id: String,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<SearchResult>, NativeProviderError> {
    let snapshot = ensure_snapshot(&app, &state, &index_state, &workspace_id)?;
    let limit = limit.unwrap_or(10).clamp(1, 50);
    let Some(space) = snapshot.embedding_space.clone() else {
        return Ok(snapshot.retriever.keyword(
            &snapshot.documents,
            &snapshot.chunks,
            &query,
            limit,
        ));
    };
    let store = model_store(&app)?;
    let provider = optional_embedding_provider(&store)?.ok_or_else(|| NativeProviderError {
        code: folio_core::contracts::ProviderErrorCode::ModelNotInstalled,
        message: "The selected embedding model is no longer installed.".into(),
        detail: None,
    })?;
    let query_vector = provider
        .embed(&[query.clone()], EmbeddingKind::Query, None)
        .map_err(native_error)?
        .into_iter()
        .next()
        .ok_or_else(|| NativeProviderError {
            code: folio_core::contracts::ProviderErrorCode::IoError,
            message: "The embedding provider returned no query vector.".into(),
            detail: None,
        })?;
    provider.unload().map_err(native_error)?;
    snapshot
        .retriever
        .search(
            &snapshot.documents,
            &snapshot.chunks,
            &query,
            Some((&space, &query_vector)),
            limit,
        )
        .map_err(native_error)
}

fn generation_provider(
    app: &AppHandle,
    generation_state: &State<'_, GenerationState>,
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
    generation_state: &State<'_, GenerationState>,
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
    generation_state: &State<'_, GenerationState>,
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
        cancel.store(true, std::sync::atomic::Ordering::Release);
    }
    Ok(())
}

#[tauri::command]
fn summarize_document(
    app: AppHandle,
    state: State<'_, WorkspaceState>,
    generation_state: State<'_, GenerationState>,
    workspace_id: String,
    document_id: String,
) -> Result<GroundedAnswer, NativeProviderError> {
    let root = authorized(&state, &workspace_id).map_err(|message| NativeProviderError {
        code: folio_core::contracts::ProviderErrorCode::IoError,
        message,
        detail: None,
    })?;
    let content =
        workspace::read_text(&root.path, &document_id).map_err(|message| NativeProviderError {
            code: folio_core::contracts::ProviderErrorCode::IoError,
            message,
            detail: None,
        })?;
    let record = DocumentRecord {
        id: document_id.clone(),
        relative_path: document_id.clone(),
        name: Path::new(&document_id)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or(&document_id)
            .into(),
        title: document_id.clone(),
        language: grounding::detect_language(&content),
        size_bytes: content.len() as u64,
        content: Some(content.clone()),
        content_hash: Some(sha256(&content)),
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
}

#[tauri::command]
fn answer_question(
    app: AppHandle,
    workspace_state: State<'_, WorkspaceState>,
    index_state: State<'_, IndexState>,
    generation_state: State<'_, GenerationState>,
    workspace_id: String,
    question: String,
    document_id: Option<String>,
) -> Result<GroundedAnswer, NativeProviderError> {
    let snapshot = ensure_snapshot(&app, &workspace_state, &index_state, &workspace_id)?;
    let results = search_snapshot(&app, &snapshot, &question)?;
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
}

fn search_snapshot(
    app: &AppHandle,
    snapshot: &IndexSnapshot,
    query: &str,
) -> Result<Vec<SearchResult>, NativeProviderError> {
    let limit = folio_core::generation::MAX_PASSAGES;
    let Some(space) = snapshot.embedding_space.clone() else {
        return Ok(snapshot
            .retriever
            .keyword(&snapshot.documents, &snapshot.chunks, query, limit));
    };
    let store = model_store(app)?;
    let provider = optional_embedding_provider(&store)?.ok_or_else(|| NativeProviderError {
        code: folio_core::contracts::ProviderErrorCode::ModelNotInstalled,
        message: "The selected embedding model is no longer installed.".into(),
        detail: None,
    })?;
    let vector = provider
        .embed(&[query.to_owned()], EmbeddingKind::Query, None)
        .map_err(native_error)?
        .into_iter()
        .next()
        .ok_or_else(|| NativeProviderError {
            code: folio_core::contracts::ProviderErrorCode::IoError,
            message: "The embedding provider returned no query vector.".into(),
            detail: None,
        })?;
    provider.unload().map_err(native_error)?;
    snapshot
        .retriever
        .search(
            &snapshot.documents,
            &snapshot.chunks,
            query,
            Some((&space, &vector)),
            limit,
        )
        .map_err(native_error)
}

#[tauri::command]
fn interpret_request(
    app: AppHandle,
    state: State<'_, WorkspaceState>,
    generation_state: State<'_, GenerationState>,
    workspace_id: String,
    text: String,
) -> Result<InterpretationResult, NativeProviderError> {
    let root = authorized(&state, &workspace_id).map_err(|message| NativeProviderError {
        code: folio_core::contracts::ProviderErrorCode::IoError,
        message,
        detail: None,
    })?;
    let (documents, contents, chunks) =
        load_corpus(&root.path).map_err(|message| NativeProviderError {
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
}

#[tauri::command]
fn unload_generation(
    generation_state: State<'_, GenerationState>,
) -> Result<(), NativeProviderError> {
    let mut guard = generation_state.lock().map_err(|_| NativeProviderError {
        code: folio_core::contracts::ProviderErrorCode::IoError,
        message: "The local generation state is unavailable.".into(),
        detail: None,
    })?;
    if let Some(cancel) = guard.active_cancel.take() {
        cancel.store(true, std::sync::atomic::Ordering::Release);
    }
    if let Some(slot) = guard.slot.take() {
        slot.provider.unload().map_err(native_error)?;
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(WorkspaceState::new(None))
        .manage(IndexState::new(None))
        .manage(Mutex::new(GenerationStateInner::default()))
        .invoke_handler(tauri::generate_handler![
            choose_workspace,
            list_documents,
            read_document,
            list_models,
            verify_model,
            install_model,
            remove_model,
            select_model,
            runtime_status,
            install_runtime,
            rebuild_index,
            index_status,
            semantic_search,
            summarize_document,
            answer_question,
            interpret_request,
            cancel_generation,
            unload_generation
        ])
        .run(tauri::generate_context!())
        .expect("Folio could not start");
}

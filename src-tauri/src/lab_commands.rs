//! Model Lab's native commands: thin adapters over `folio_core::lab` and
//! `lab_store`. The lab never gets a user folder: it measures a disposable copy
//! of the bundled corpus under app data, one generation model at a time, and
//! holds the generation slot for the whole run so user generation is refused
//! with `providerBusy` instead of competing for memory.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use folio_core::contracts::{
    ModelDescriptor, ModelInstallStatus, ModelRole, NativeProviderError, ProviderErrorCode,
};
use folio_core::generation::GenerationProvider;
use folio_core::lab::host::{host_info, llama_server_version, onnxruntime_version};
use folio_core::lab::native::{model_ref, open_embedding, StoreGeneratorFactory};
use folio_core::lab::runner::{
    system_clock_ms, EmbeddingSubject, LabProgress, LabRunner, OsMemoryProbe, RunEnd,
};
use folio_core::lab::suite::{Corpus, Suite};
use folio_core::lab::workspace::LabWorkspaces;
use folio_core::lab::{
    BenchmarkRecord, BenchmarkTask, ReviewStatus, RunSummary, RuntimeDetail, RuntimeName,
};
use folio_core::models::ModelStore;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter, State};

use super::{
    app_data_dir, finish_generation, model_store, native_error, run_blocking, runtime_id_for_host,
    unload_embedding, EmbeddingState, Folio, GenerationState,
};
use crate::error::{error, ErrorCode, FolioError};
use crate::lab_store::{self, RecordFilter, ReviewInput, SqliteLabSink};

const LAB_PROGRESS_EVENT: &str = "folio://lab-progress";

/// The cancel flag of the lab run in progress, if any. It is the same flag the
/// generation state holds as its active request, so unloading generation
/// (including at exit) also stops the lab.
pub(crate) type LabState = Arc<Mutex<Option<Arc<AtomicBool>>>>;

/// A manifest model and whether the lab could run it now. `modelFileBytes` is
/// the pinned size of the model's own files, not an installed size.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LabModel {
    id: String,
    role: ModelRole,
    repo: String,
    revision: String,
    quantization: String,
    model_file_bytes: u64,
    status: ModelInstallStatus,
    /// Installed and hash-verified, so the lab may run it.
    runnable: bool,
    selected: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunLabRequest {
    embedding_model_id: String,
    generation_model_ids: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunStarted {
    run_id: String,
}

fn busy(message: &str) -> NativeProviderError {
    NativeProviderError {
        code: ProviderErrorCode::GenerationBusy,
        message: message.into(),
        detail: None,
    }
}

fn unavailable() -> NativeProviderError {
    NativeProviderError {
        code: ProviderErrorCode::IoError,
        message: "The Model Lab state is unavailable.".into(),
        detail: None,
    }
}

fn invalid_selection(message: &str) -> FolioError {
    error(ErrorCode::Internal, message).with_detail("reportedCode", "labSelectionInvalid")
}

fn describe_models(store: &ModelStore) -> Result<Vec<LabModel>, FolioError> {
    let selected_embedding = store.selected_model(ModelRole::Embedding)?;
    let selected_generation = store.selected_model(ModelRole::Generation)?;
    let mut models = Vec::new();
    for descriptor in &store.manifest().models {
        let status = store.model_state(&descriptor.id)?.status;
        let selected = match descriptor.role {
            ModelRole::Embedding => selected_embedding.as_deref() == Some(descriptor.id.as_str()),
            ModelRole::Generation => selected_generation.as_deref() == Some(descriptor.id.as_str()),
        };
        models.push(LabModel {
            id: descriptor.id.clone(),
            role: descriptor.role.clone(),
            repo: descriptor.repo.clone(),
            revision: descriptor.revision.clone(),
            quantization: descriptor.quantization.clone(),
            model_file_bytes: descriptor.files.iter().map(|file| file.bytes).sum(),
            runnable: status == ModelInstallStatus::Installed,
            status,
            selected,
        });
    }
    Ok(models)
}

fn require_runnable(
    store: &ModelStore,
    id: &str,
    role: ModelRole,
) -> Result<ModelDescriptor, FolioError> {
    let descriptor = store.model(id)?.clone();
    if descriptor.role != role {
        return Err(
            invalid_selection("A model was given for the wrong role.").with_detail("modelId", id)
        );
    }
    if store.model_state(id)?.status != ModelInstallStatus::Installed {
        return Err(NativeProviderError {
            code: ProviderErrorCode::ModelNotInstalled,
            message: "That model is not installed and verified, so Model Lab cannot run it.".into(),
            detail: Some(id.into()),
        }
        .into());
    }
    Ok(descriptor)
}

/// Only installed, hash-verified models of the right role, each once, in the
/// order given. There is no substitution: a model that cannot run stops the
/// request.
fn check_lab_selection(
    store: &ModelStore,
    embedding_id: &str,
    generation_ids: &[String],
) -> Result<(), FolioError> {
    if generation_ids.is_empty() {
        return Err(invalid_selection("Choose at least one generation model."));
    }
    for (index, id) in generation_ids.iter().enumerate() {
        if generation_ids[..index].contains(id) {
            return Err(invalid_selection("A generation model was chosen twice.")
                .with_detail("modelId", id.as_str()));
        }
    }
    require_runnable(store, embedding_id, ModelRole::Embedding)?;
    for id in generation_ids {
        require_runnable(store, id, ModelRole::Generation)?;
    }
    Ok(())
}

/// Takes the generation slot for the whole run and returns its cancel flag.
fn begin_lab_exclusive(
    generation_state: &GenerationState,
    lab_state: &LabState,
) -> Result<Arc<AtomicBool>, NativeProviderError> {
    let mut lab = lab_state.lock().map_err(|_| unavailable())?;
    if lab.is_some() {
        return Err(busy("A Model Lab run is already in progress."));
    }
    let mut guard = generation_state.lock().map_err(|_| unavailable())?;
    if guard.active_cancel.is_some() {
        return Err(busy("Another local generation request is active."));
    }
    if guard.runtime_installing {
        return Err(busy(
            "The local AI runtime is being installed. Try again when it finishes.",
        ));
    }
    if let Some(slot) = guard.slot.take() {
        slot.provider.unload().map_err(native_error)?;
    }
    let cancel = Arc::new(AtomicBool::new(false));
    guard.active_cancel = Some(cancel.clone());
    *lab = Some(cancel.clone());
    Ok(cancel)
}

fn finish_lab(
    generation_state: &GenerationState,
    lab_state: &LabState,
    cancel: &Arc<AtomicBool>,
) -> Result<(), NativeProviderError> {
    finish_generation(generation_state, cancel)?;
    let mut lab = lab_state.lock().map_err(|_| unavailable())?;
    if lab
        .as_ref()
        .is_some_and(|active| Arc::ptr_eq(active, cancel))
    {
        *lab = None;
    }
    Ok(())
}

fn execute_lab(
    app: &AppHandle,
    index_path: &std::path::Path,
    request: &RunLabRequest,
    run_id: &str,
    cancel: &AtomicBool,
) -> Result<RunEnd, FolioError> {
    let data_dir = app_data_dir(app).map_err(|failure| FolioError::from(failure))?;
    let store = ModelStore::new(&data_dir)?;
    let executable = store
        .verified_runtime_executable(runtime_id_for_host())
        .map_err(native_error)?;
    let runtime_version = llama_server_version(&executable)?;
    let embedding_descriptor = store.model(&request.embedding_model_id)?.clone();
    let provider = open_embedding(&store, &embedding_descriptor)?;
    let threads = std::thread::available_parallelism()
        .map(|value| value.get().saturating_sub(1).max(1))
        .unwrap_or(1);

    let mut connection = crate::db::open(index_path)?;
    lab_store::fail_interrupted_runs(&mut connection, system_clock_ms())?;
    let mut sink = SqliteLabSink::new(connection);

    let workspaces = LabWorkspaces::new(&data_dir);
    let suite = Suite::embedded()?;
    let corpus = Corpus::embedded();
    let factory = StoreGeneratorFactory {
        data_dir: data_dir.clone(),
        executable,
        runtime: RuntimeDetail {
            name: RuntimeName::LlamaCpp,
            version: runtime_version,
        },
        threads,
    };
    let emit = |progress: &LabProgress| {
        let _ = app.emit(LAB_PROGRESS_EVENT, progress);
    };
    Ok(LabRunner {
        run_id: run_id.to_string(),
        workspaces: &workspaces,
        suite: &suite,
        corpus: &corpus,
        embedding: EmbeddingSubject {
            model: model_ref(&embedding_descriptor),
            runtime: RuntimeDetail {
                name: RuntimeName::OnnxRuntime,
                version: onnxruntime_version(),
            },
            provider: &provider,
        },
        generation_model_ids: request.generation_model_ids.clone(),
        factory: &factory,
        probe: &OsMemoryProbe,
        sink: &mut sink,
        cancel,
        threads: threads as u32,
        host: host_info(),
        clock_ms: system_clock_ms,
        progress: Some(&emit),
    }
    .run()?)
}

#[tauri::command]
pub(crate) async fn lab_models(app: AppHandle) -> Result<Vec<LabModel>, FolioError> {
    run_blocking::<_, FolioError, _>(move || describe_models(&model_store(&app)?)).await
}

/// Starts a run and returns its id at once; progress arrives as
/// `folio://lab-progress` events and results through `list_lab_results`.
#[tauri::command]
pub(crate) async fn run_model_lab(
    app: AppHandle,
    state: State<'_, Folio>,
    generation_state: State<'_, GenerationState>,
    embedding_state: State<'_, EmbeddingState>,
    lab_state: State<'_, LabState>,
    request: RunLabRequest,
) -> Result<RunStarted, FolioError> {
    let index_path = state.index_path.clone();
    let generation_state = generation_state.inner().clone();
    let embedding_state = embedding_state.inner().clone();
    let lab_state = lab_state.inner().clone();
    run_blocking::<_, FolioError, _>(move || {
        let store = model_store(&app)?;
        check_lab_selection(&store, &request.embedding_model_id, &request.generation_model_ids)?;
        store
            .verified_runtime_executable(runtime_id_for_host())
            .map_err(native_error)?;
        let cancel = begin_lab_exclusive(&generation_state, &lab_state)?;
        if let Err(failure) = unload_embedding(&embedding_state) {
            let _ = finish_lab(&generation_state, &lab_state, &cancel);
            return Err(failure.into());
        }
        let run_id = format!("lab-{}", system_clock_ms());
        let worker = {
            let (app, run_id, cancel) = (app.clone(), run_id.clone(), cancel.clone());
            let (generation_state, lab_state) = (generation_state.clone(), lab_state.clone());
            move || {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    execute_lab(&app, &index_path, &request, &run_id, &cancel)
                }));
                let (step, failure) = match outcome {
                    Ok(Ok(RunEnd::Completed)) => ("finished", None),
                    Ok(Ok(RunEnd::Cancelled)) => ("cancelled", None),
                    Ok(Err(failure)) => ("failed", Some(failure.message)),
                    Err(_) => ("failed", Some("The Model Lab run stopped unexpectedly.".to_string())),
                };
                let _ = app.emit(
                    LAB_PROGRESS_EVENT,
                    json!({ "runId": run_id, "step": step, "caseId": null, "modelId": null, "error": failure }),
                );
                let _ = finish_lab(&generation_state, &lab_state, &cancel);
            }
        };
        if let Err(cause) = std::thread::Builder::new().name("folio-model-lab".into()).spawn(worker) {
            let _ = finish_lab(&generation_state, &lab_state, &cancel);
            return Err(error(ErrorCode::Internal, "Model Lab could not start.")
                .with_detail("cause", cause.to_string()));
        }
        Ok(RunStarted { run_id })
    })
    .await
}

/// Stops between cases. Finished results are kept and the run is marked cancelled.
#[tauri::command]
pub(crate) fn cancel_model_lab(lab_state: State<'_, LabState>) -> Result<(), FolioError> {
    let lab = lab_state
        .lock()
        .map_err(|_| FolioError::from(unavailable()))?;
    if let Some(cancel) = lab.as_ref() {
        cancel.store(true, Ordering::Release);
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn list_lab_runs(state: State<'_, Folio>) -> Result<Vec<RunSummary>, FolioError> {
    lab_store::list_runs(&*state.index()?)
}

#[tauri::command]
pub(crate) async fn list_lab_results(
    state: State<'_, Folio>,
    run_id: Option<String>,
    model_id: Option<String>,
    task: Option<BenchmarkTask>,
) -> Result<Vec<BenchmarkRecord>, FolioError> {
    lab_store::list_records(
        &*state.index()?,
        &RecordFilter {
            run_id,
            model_id,
            task,
        },
    )
}

/// Appends a person's review. It must name the output hash the reviewer read.
#[tauri::command]
pub(crate) async fn record_lab_review(
    state: State<'_, Folio>,
    id: String,
    output_sha256: String,
    status: ReviewStatus,
    reviewer: String,
    notes: Option<String>,
) -> Result<BenchmarkRecord, FolioError> {
    lab_store::record_review(
        &mut *state.index()?,
        &id,
        &output_sha256,
        ReviewInput {
            status,
            reviewer,
            notes,
        },
        system_clock_ms(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, ModelStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path()).unwrap();
        (dir, store)
    }

    fn generation_ids(store: &ModelStore) -> Vec<String> {
        store
            .manifest()
            .models
            .iter()
            .filter(|model| model.role == ModelRole::Generation)
            .map(|model| model.id.clone())
            .collect()
    }

    #[test]
    fn the_model_list_reports_pinned_file_sizes_and_nothing_runnable_before_install() {
        let (_dir, store) = store();
        let models = describe_models(&store).unwrap();
        assert_eq!(models.len(), store.manifest().models.len());
        assert!(models
            .iter()
            .any(|model| model.role == ModelRole::Embedding));
        for model in &models {
            assert!(!model.runnable, "{} is not installed", model.id);
            assert_eq!(model.status, ModelInstallStatus::NotInstalled);
            assert!(!model.selected);
            let pinned: u64 = store
                .model(&model.id)
                .unwrap()
                .files
                .iter()
                .map(|file| file.bytes)
                .sum();
            assert_eq!(model.model_file_bytes, pinned);
        }
    }

    #[test]
    fn a_selection_of_models_that_are_not_installed_is_refused_not_substituted() {
        let (_dir, store) = store();
        let generation = generation_ids(&store);
        let embedding = store
            .manifest()
            .models
            .iter()
            .find(|model| model.role == ModelRole::Embedding)
            .unwrap()
            .id
            .clone();
        let failure = check_lab_selection(&store, &embedding, &generation[..1]).unwrap_err();
        assert_eq!(failure.code, ErrorCode::ModelNotInstalled);
        assert_eq!(failure.detail("modelId"), Some(embedding.as_str()));
    }

    #[test]
    fn an_empty_duplicate_or_wrong_role_selection_is_refused() {
        let (_dir, store) = store();
        let generation = generation_ids(&store);
        let embedding = store
            .manifest()
            .models
            .iter()
            .find(|model| model.role == ModelRole::Embedding)
            .unwrap()
            .id
            .clone();
        let empty = check_lab_selection(&store, &embedding, &[]).unwrap_err();
        assert_eq!(empty.detail("reportedCode"), Some("labSelectionInvalid"));

        let twice = vec![generation[0].clone(), generation[0].clone()];
        let duplicate = check_lab_selection(&store, &embedding, &twice).unwrap_err();
        assert_eq!(
            duplicate.detail("reportedCode"),
            Some("labSelectionInvalid")
        );

        let wrong_role =
            check_lab_selection(&store, &generation[0], &generation[1..2]).unwrap_err();
        assert_eq!(
            wrong_role.detail("reportedCode"),
            Some("labSelectionInvalid")
        );

        assert!(check_lab_selection(&store, &embedding, &["no-such-model".to_string()]).is_err());
    }

    #[test]
    fn a_lab_run_holds_the_generation_slot_and_only_one_run_can_hold_it() {
        let generation = GenerationState::default();
        let lab = LabState::default();
        let cancel = begin_lab_exclusive(&generation, &lab).unwrap();

        let second = begin_lab_exclusive(&generation, &lab).unwrap_err();
        assert_eq!(second.code, ProviderErrorCode::GenerationBusy);
        // User generation sees the slot as busy while the lab runs.
        assert!(generation.lock().unwrap().active_cancel.is_some());

        finish_lab(&generation, &lab, &cancel).unwrap();
        assert!(generation.lock().unwrap().active_cancel.is_none());
        assert!(lab.lock().unwrap().is_none());
        begin_lab_exclusive(&generation, &lab).unwrap();
    }

    #[test]
    fn a_lab_run_is_refused_while_a_generation_request_is_active() {
        let generation = GenerationState::default();
        let lab = LabState::default();
        generation.lock().unwrap().active_cancel = Some(Arc::new(AtomicBool::new(false)));
        let failure = begin_lab_exclusive(&generation, &lab).unwrap_err();
        assert_eq!(failure.code, ProviderErrorCode::GenerationBusy);
        assert!(lab.lock().unwrap().is_none(), "a refused run holds nothing");

        let installing = GenerationState::default();
        installing.lock().unwrap().runtime_installing = true;
        assert!(begin_lab_exclusive(&installing, &lab).is_err());
    }

    #[test]
    fn unloading_generation_cancels_the_lab_run() {
        let generation = GenerationState::default();
        let lab = LabState::default();
        let cancel = begin_lab_exclusive(&generation, &lab).unwrap();
        assert!(!cancel.load(Ordering::Acquire));
        crate::unload_generation_now(&generation).unwrap();
        assert!(cancel.load(Ordering::Acquire));
    }
}

//! Model Lab's native commands: thin adapters over `folio_core::lab` and
//! `lab_store`. The lab never gets a user folder: it measures a disposable copy
//! of the bundled corpus under app data, one generation model at a time, and
//! holds the generation slot for the whole run so user generation is refused
//! with `providerBusy` instead of competing for memory.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use folio_core::contracts::{
    ModelDescriptor, ModelInstallStatus, ModelRole, NativeProviderError, ProviderErrorCode,
};
use folio_core::generation::GenerationProvider;
use folio_core::lab::candidates::{candidate_store, is_candidate_id, license_note};
use folio_core::lab::host::{host_info, onnxruntime_version};
use folio_core::lab::native::{
    cpu_only_options, llama_runtime_detail, model_ref, open_embedding, StoreGeneratorFactory,
};
use folio_core::lab::runner::{
    system_clock_ms, EmbeddingSubject, LabProgress, LabRunner, OsMemoryProbe, RunEnd,
};
use folio_core::lab::suite::{Corpus, Suite};
use folio_core::lab::workspace::LabWorkspaces;
use folio_core::lab::{
    BenchmarkRecord, BenchmarkTask, GpuOffload, ModelCatalog, ReviewStatus, RunSummary,
    RuntimeDetail, RuntimeName,
};
use folio_core::models::ModelStore;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter, State};

use super::{
    app_data_dir, begin_install, finish_generation, finish_install, model_store, native_error,
    provider_install_state, run_blocking, runtime_id_for_host, unload_embedding, EmbeddingState,
    Folio, GenerationState, InstallState, ProviderInstallState,
};
use crate::error::{error, ErrorCode, FolioError};
use crate::lab_store::{self, RecordFilter, ReviewInput, SqliteLabSink};

const LAB_PROGRESS_EVENT: &str = "folio://lab-progress";

/// The cancel flag of the lab run in progress, if any. It is the same flag the
/// generation state holds as its active request, so unloading generation
/// (including at exit) also stops the lab.
///
/// A distinct type, not an alias: Tauri keeps one managed state per type, and
/// `InstallState` has the same inner type. An alias made `.manage` panic at
/// startup, so the app never opened.
#[derive(Clone, Default)]
pub(crate) struct LabState(Arc<Mutex<Option<Arc<AtomicBool>>>>);

impl std::ops::Deref for LabState {
    type Target = Mutex<Option<Arc<AtomicBool>>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

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
    /// Only a product model can be the app's selection.
    selected: bool,
    catalog: ModelCatalog,
    /// An evaluation candidate is not supported, recommended or selectable.
    evaluation_only: bool,
    license: String,
    /// A caveat on the license, e.g. conflicting publisher metadata.
    license_note: Option<String>,
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

fn describe_models(
    product: &ModelStore,
    candidates: &ModelStore,
) -> Result<Vec<LabModel>, FolioError> {
    let selected_embedding = product.selected_model(ModelRole::Embedding)?;
    let selected_generation = product.selected_model(ModelRole::Generation)?;
    let mut models = Vec::new();
    for (store, catalog) in [
        (product, ModelCatalog::Product),
        (candidates, ModelCatalog::EvaluationCandidate),
    ] {
        let candidate = catalog == ModelCatalog::EvaluationCandidate;
        for descriptor in &store.manifest().models {
            let status = store.model_state(&descriptor.id)?.status;
            let selected = !candidate
                && match descriptor.role {
                    ModelRole::Embedding => {
                        selected_embedding.as_deref() == Some(descriptor.id.as_str())
                    }
                    ModelRole::Generation => {
                        selected_generation.as_deref() == Some(descriptor.id.as_str())
                    }
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
                catalog,
                evaluation_only: candidate,
                license: descriptor.license.clone(),
                license_note: if candidate {
                    license_note(&descriptor.id)
                } else {
                    None
                },
            });
        }
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

/// The store that holds a generation model: the candidate store for a
/// candidate id, the product store otherwise. Ids never collide.
fn store_for<'a>(product: &'a ModelStore, candidates: &'a ModelStore, id: &str) -> &'a ModelStore {
    if candidates.model(id).is_ok() {
        candidates
    } else {
        product
    }
}

/// Only installed, hash-verified models of the right role, each once, in the
/// order given. The embedding model is always a product model. There is no
/// substitution: a model that cannot run stops the request.
fn check_lab_selection(
    product: &ModelStore,
    candidates: &ModelStore,
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
    if candidates.model(embedding_id).is_ok() {
        return Err(
            invalid_selection("The embedding model must be a product model.")
                .with_detail("modelId", embedding_id),
        );
    }
    require_runnable(product, embedding_id, ModelRole::Embedding)?;
    for id in generation_ids {
        require_runnable(
            store_for(product, candidates, id),
            id,
            ModelRole::Generation,
        )?;
    }
    Ok(())
}

/// The isolated store for an evaluation candidate. A product id, or an id in
/// neither catalog, is refused so the candidate commands can never reach a
/// product model.
fn candidate_store_for(data_dir: &std::path::Path, id: &str) -> Result<ModelStore, FolioError> {
    if !is_candidate_id(id) {
        return Err(
            invalid_selection("That is not an evaluation candidate.").with_detail("modelId", id)
        );
    }
    Ok(candidate_store(data_dir)?)
}

fn verify_candidate(
    data_dir: &std::path::Path,
    id: &str,
) -> Result<folio_core::contracts::ModelInstallState, FolioError> {
    Ok(candidate_store_for(data_dir, id)?.model_state(id)?)
}

fn remove_candidate(data_dir: &std::path::Path, id: &str) -> Result<(), FolioError> {
    Ok(candidate_store_for(data_dir, id)?.remove_model(id)?)
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

/// What a lab run holds for its whole duration: the generation slot (so user
/// generation gets `providerBusy`) and the install lock (so no model or runtime
/// can be installed, removed, re-selected or replaced under it).
#[derive(Clone, Debug)]
struct LabHold {
    cancel: Arc<AtomicBool>,
    install_lock: Arc<AtomicBool>,
}

fn begin_lab_run(
    install_state: &InstallState,
    generation_state: &GenerationState,
    lab_state: &LabState,
) -> Result<LabHold, NativeProviderError> {
    let install_lock = begin_install(install_state)?;
    match begin_lab_exclusive(generation_state, lab_state) {
        Ok(cancel) => Ok(LabHold {
            cancel,
            install_lock,
        }),
        Err(failure) => {
            let _ = finish_install(install_state, &install_lock);
            Err(failure)
        }
    }
}

/// Releases everything `begin_lab_run` took, on every exit path.
fn end_lab_run(
    install_state: &InstallState,
    generation_state: &GenerationState,
    lab_state: &LabState,
    hold: &LabHold,
) {
    let _ = finish_lab(generation_state, lab_state, &hold.cancel);
    let _ = finish_install(install_state, &hold.install_lock);
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
    let llama_runtime = llama_runtime_detail(
        &store,
        runtime_id_for_host(),
        &executable,
        GpuOffload::Disabled,
        cpu_only_options().extra_args(),
    )?;
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
        runtime: llama_runtime,
        threads,
        // The measurement target is CPU inference; the server's own output
        // is captured so the backend it chose is recorded, not assumed.
        cpu_only: true,
        log_dir: Some(workspaces.logs_dir(run_id)?),
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
                backend: None,
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
    run_blocking::<_, FolioError, _>(move || {
        let data_dir = app_data_dir(&app)?;
        describe_models(&model_store(&app)?, &candidate_store(&data_dir)?)
    })
    .await
}

/// Downloads one evaluation candidate into its own folder, only when asked.
/// Sizes and SHA-256 are verified like any model; nothing is selected.
#[tauri::command]
pub(crate) async fn install_lab_candidate(
    app: AppHandle,
    install_state: State<'_, InstallState>,
    model_id: String,
) -> Result<ProviderInstallState, FolioError> {
    let cancel = begin_install(install_state.inner())?;
    let worker_cancel = cancel.clone();
    let install_state = install_state.inner().clone();
    let progress_app = app.clone();
    let result = run_blocking::<_, FolioError, _>(move || {
        let store = candidate_store_for(&app_data_dir(&app)?, &model_id)?;
        Ok(store.install_model(&model_id, &worker_cancel, |progress| {
            let _ = progress_app.emit("folio://model-progress", progress);
        })?)
    })
    .await;
    finish_install(&install_state, &cancel)?;
    Ok(provider_install_state(result?))
}

/// Removes one evaluation candidate. Product models and recorded results are
/// never touched.
#[tauri::command]
pub(crate) async fn remove_lab_candidate(
    app: AppHandle,
    install_state: State<'_, InstallState>,
    model_id: String,
) -> Result<(), FolioError> {
    let lock = begin_install(install_state.inner())?;
    let install_state = install_state.inner().clone();
    let result =
        run_blocking::<_, FolioError, _>(move || remove_candidate(&app_data_dir(&app)?, &model_id))
            .await;
    finish_install(&install_state, &lock)?;
    result
}

#[tauri::command]
pub(crate) async fn verify_lab_candidate(
    app: AppHandle,
    model_id: String,
) -> Result<ProviderInstallState, FolioError> {
    let state =
        run_blocking::<_, FolioError, _>(move || verify_candidate(&app_data_dir(&app)?, &model_id))
            .await?;
    Ok(provider_install_state(state))
}

/// Starts a run and returns its id at once; progress arrives as
/// `folio://lab-progress` events and results through `list_lab_results`.
#[tauri::command]
pub(crate) async fn run_model_lab(
    app: AppHandle,
    state: State<'_, Folio>,
    generation_state: State<'_, GenerationState>,
    embedding_state: State<'_, EmbeddingState>,
    install_state: State<'_, InstallState>,
    lab_state: State<'_, LabState>,
    request: RunLabRequest,
) -> Result<RunStarted, FolioError> {
    let index_path = state.index_path.clone();
    let generation_state = generation_state.inner().clone();
    let embedding_state = embedding_state.inner().clone();
    let lab_state = lab_state.inner().clone();
    let install_state = install_state.inner().clone();
    run_blocking::<_, FolioError, _>(move || {
        let store = model_store(&app)?;
        let candidates = candidate_store(&app_data_dir(&app)?)?;
        check_lab_selection(
            &store,
            &candidates,
            &request.embedding_model_id,
            &request.generation_model_ids,
        )?;
        store
            .verified_runtime_executable(runtime_id_for_host())
            .map_err(native_error)?;
        let hold = begin_lab_run(&install_state, &generation_state, &lab_state)?;
        if let Err(failure) = unload_embedding(&embedding_state) {
            end_lab_run(&install_state, &generation_state, &lab_state, &hold);
            return Err(failure.into());
        }
        let run_id = format!("lab-{}", system_clock_ms());
        let worker = {
            let (app, run_id, hold) = (app.clone(), run_id.clone(), hold.clone());
            let (generation_state, lab_state) = (generation_state.clone(), lab_state.clone());
            let install_state = install_state.clone();
            move || {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    execute_lab(&app, &index_path, &request, &run_id, &hold.cancel)
                }));
                let (step, failure) = match outcome {
                    Ok(Ok(RunEnd::Completed)) => ("finished", None),
                    Ok(Ok(RunEnd::Cancelled)) => ("cancelled", None),
                    Ok(Err(failure)) => ("failed", Some(failure.message)),
                    Err(_) => (
                        "failed",
                        Some("The Model Lab run stopped unexpectedly.".to_string()),
                    ),
                };
                let _ = app.emit(
                    LAB_PROGRESS_EVENT,
                    json!({ "runId": run_id, "step": step, "caseId": null, "modelId": null, "error": failure }),
                );
                end_lab_run(&install_state, &generation_state, &lab_state, &hold);
            }
        };
        if let Err(cause) = std::thread::Builder::new()
            .name("folio-model-lab".into())
            .spawn(worker)
        {
            end_lab_run(&install_state, &generation_state, &lab_state, &hold);
            return Err(error(ErrorCode::Internal, "Model Lab could not start.")
                .with_detail("cause", cause.to_string()));
        }
        Ok(RunStarted { run_id })
    })
    .await
}

/// Marks runs an earlier session left `running` as interrupted. Called once at
/// startup, when no run can be in progress, so the list never shows a run that
/// will not end.
pub(crate) fn mark_interrupted_runs(state: &Folio) {
    let marked = state.index().and_then(|mut connection| {
        lab_store::fail_interrupted_runs(&mut connection, system_clock_ms())
    });
    if let Err(failure) = marked {
        eprintln!(
            "Model Lab could not mark interrupted runs: {}",
            failure.message
        );
    }
}

/// Asks a lab run to stop and waits up to `limit` for it to end. Its
/// llama-server stops when the run ends; without this wait, quitting during a
/// run could leave that server running on macOS and Linux, where no job object
/// ties it to Folio. Returns whether no run is left.
pub(crate) fn stop_lab_and_wait(lab_state: &LabState, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    loop {
        match lab_state.lock() {
            Ok(lab) => match lab.as_ref() {
                None => return true,
                Some(cancel) => cancel.store(true, Ordering::Release),
            },
            Err(_) => return false,
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
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

    fn stores() -> (tempfile::TempDir, ModelStore, ModelStore) {
        let dir = tempfile::tempdir().unwrap();
        let product = ModelStore::new(dir.path()).unwrap();
        let candidates = candidate_store(dir.path()).unwrap();
        (dir, product, candidates)
    }

    fn ids(store: &ModelStore, role: ModelRole) -> Vec<String> {
        store
            .manifest()
            .models
            .iter()
            .filter(|model| model.role == role)
            .map(|model| model.id.clone())
            .collect()
    }

    #[test]
    fn the_model_list_flags_candidates_and_reports_pinned_sizes_not_installed_size() {
        let (_dir, product, candidates) = stores();
        let models = describe_models(&product, &candidates).unwrap();
        assert_eq!(
            models.len(),
            product.manifest().models.len() + candidates.manifest().models.len()
        );
        for model in &models {
            assert!(!model.runnable, "{} is not installed", model.id);
            assert_eq!(model.status, ModelInstallStatus::NotInstalled);
            assert!(!model.selected);
            let in_candidates = candidates.model(&model.id).is_ok();
            assert_eq!(model.evaluation_only, in_candidates, "{}", model.id);
            assert_eq!(
                model.catalog == ModelCatalog::EvaluationCandidate,
                in_candidates
            );
            let store = if in_candidates { &candidates } else { &product };
            let pinned: u64 = store
                .model(&model.id)
                .unwrap()
                .files
                .iter()
                .map(|file| file.bytes)
                .sum();
            assert_eq!(model.model_file_bytes, pinned);
        }
        let sea = models
            .iter()
            .find(|model| model.id == "gemma-sea-lion-v4.5-e2b-q4-k-m")
            .unwrap();
        assert!(sea.license_note.as_deref().unwrap().contains("Unsettled"));
        assert!(models
            .iter()
            .filter(|model| !model.evaluation_only)
            .all(|model| model.license_note.is_none()));
    }

    #[test]
    fn a_candidate_is_never_the_apps_selection_even_if_it_is_installed_in_the_lab() {
        let (_dir, product, candidates) = stores();
        for role in [ModelRole::Generation, ModelRole::Embedding] {
            assert!(product
                .select_model(role.clone(), "qwen3.5-0.8b-q4-k-m")
                .is_err());
        }
        assert!(product.model("qwen3.5-0.8b-q4-k-m").is_err());
        // The candidate store's own settings are never read as the product selection.
        assert_eq!(product.selected_model(ModelRole::Generation).unwrap(), None);
        assert_eq!(
            candidates.selected_model(ModelRole::Generation).unwrap(),
            None
        );
    }

    fn first_embedding(product: &ModelStore) -> String {
        ids(product, ModelRole::Embedding).remove(0)
    }

    #[test]
    fn a_selection_of_models_that_are_not_installed_is_refused_not_substituted() {
        let (_dir, product, candidates) = stores();
        let embedding = first_embedding(&product);
        let generation = ids(&product, ModelRole::Generation);
        let failure =
            check_lab_selection(&product, &candidates, &embedding, &generation[..1]).unwrap_err();
        assert_eq!(failure.code, ErrorCode::ModelNotInstalled);
        assert_eq!(failure.detail("modelId"), Some(embedding.as_str()));
    }

    #[test]
    fn an_uninstalled_candidate_stops_the_request_naming_it() {
        let (_dir, product, candidates) = stores();
        // A candidate is never an embedding model.
        let wrong_role = check_lab_selection(
            &product,
            &candidates,
            "qwen3.5-0.8b-q4-k-m",
            &["qwen3-0.6b-q4-k-m".to_string()],
        )
        .unwrap_err();
        assert_eq!(
            wrong_role.detail("reportedCode"),
            Some("labSelectionInvalid")
        );
        assert_eq!(wrong_role.detail("modelId"), Some("qwen3.5-0.8b-q4-k-m"));
        let failure = require_runnable(&candidates, "qwen3.5-0.8b-q4-k-m", ModelRole::Generation)
            .unwrap_err();
        assert_eq!(failure.code, ErrorCode::ModelNotInstalled);
        assert_eq!(failure.detail("modelId"), Some("qwen3.5-0.8b-q4-k-m"));
    }

    #[test]
    fn an_empty_duplicate_or_wrong_role_selection_is_refused() {
        let (_dir, product, candidates) = stores();
        let generation = ids(&product, ModelRole::Generation);
        let embedding = first_embedding(&product);
        let empty = check_lab_selection(&product, &candidates, &embedding, &[]).unwrap_err();
        assert_eq!(empty.detail("reportedCode"), Some("labSelectionInvalid"));

        let twice = vec![generation[0].clone(), generation[0].clone()];
        let duplicate = check_lab_selection(&product, &candidates, &embedding, &twice).unwrap_err();
        assert_eq!(
            duplicate.detail("reportedCode"),
            Some("labSelectionInvalid")
        );

        let wrong_role =
            check_lab_selection(&product, &candidates, &generation[0], &generation[1..2])
                .unwrap_err();
        assert_eq!(
            wrong_role.detail("reportedCode"),
            Some("labSelectionInvalid")
        );

        assert!(check_lab_selection(
            &product,
            &candidates,
            &embedding,
            &["no-such-model".to_string()]
        )
        .is_err());
    }

    #[test]
    fn the_candidate_commands_never_reach_a_product_model() {
        let dir = tempfile::tempdir().unwrap();
        let product = dir.path().join("models").join("qwen3-0.6b-q4-k-m");
        std::fs::create_dir_all(&product).unwrap();
        std::fs::write(product.join("model.gguf"), b"product weights").unwrap();

        for id in [
            "qwen3-0.6b-q4-k-m",
            "multilingual-e5-small-int8",
            "no-such-model",
        ] {
            let removal = remove_candidate(dir.path(), id).unwrap_err();
            assert_eq!(
                removal.detail("reportedCode"),
                Some("labSelectionInvalid"),
                "{id}"
            );
            assert!(verify_candidate(dir.path(), id).is_err(), "{id}");
        }
        assert!(product.join("model.gguf").is_file());
    }

    #[test]
    fn removing_a_candidate_deletes_only_its_own_folder_and_leaves_results_alone() {
        let dir = tempfile::tempdir().unwrap();
        let product = dir.path().join("models").join("qwen3-0.6b-q4-k-m");
        std::fs::create_dir_all(&product).unwrap();
        std::fs::write(product.join("model.gguf"), b"product weights").unwrap();
        let candidate = dir
            .path()
            .join("model-lab")
            .join("candidates")
            .join("models")
            .join("qwen3.5-0.8b-q4-k-m");
        std::fs::create_dir_all(&candidate).unwrap();
        std::fs::write(candidate.join("model.gguf"), b"candidate weights").unwrap();

        // A recorded result lives in the index database, not in the model folder.
        let mut connection = crate::db::open(&dir.path().join("folio.sqlite")).unwrap();
        connection
            .execute(
                "INSERT INTO benchmark_results (id, case_id, task_type, model_id, conditions_json, measurements_json, created_at) VALUES ('r', 'c', 'summary', 'qwen3.5-0.8b-q4-k-m', '{}', '{}', '1')",
                [],
            )
            .unwrap();

        remove_candidate(dir.path(), "qwen3.5-0.8b-q4-k-m").unwrap();
        assert!(!candidate.exists());
        assert!(product.join("model.gguf").is_file());
        let kept: i64 = connection
            .query_row("SELECT count(*) FROM benchmark_results", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(kept, 1);
        assert_eq!(
            verify_candidate(dir.path(), "qwen3.5-0.8b-q4-k-m")
                .unwrap()
                .status,
            ModelInstallStatus::NotInstalled
        );
    }

    #[test]
    fn a_lab_run_blocks_every_install_remove_select_and_runtime_install_until_it_ends() {
        let install = InstallState::default();
        let generation = GenerationState::default();
        let lab = LabState::default();
        let hold = begin_lab_run(&install, &generation, &lab).unwrap();

        // install_model, remove_model, select_model and install_runtime all start
        // with begin_install; install_runtime also needs an idle generation slot.
        let refused = begin_install(&install).unwrap_err();
        assert_eq!(refused.code, ProviderErrorCode::GenerationBusy);
        assert!(crate::begin_runtime_install(&generation).is_err());
        // A second run cannot start, and a refused one releases nothing it did not take.
        assert!(begin_lab_run(&install, &generation, &lab).is_err());
        assert!(lab.lock().unwrap().is_some());

        end_lab_run(&install, &generation, &lab, &hold);
        let lock = begin_install(&install).expect("the install lock is released");
        finish_install(&install, &lock).unwrap();
        crate::begin_runtime_install(&generation).expect("the slot is released");
        crate::end_runtime_install(&generation);
        assert!(lab.lock().unwrap().is_none());
    }

    #[test]
    fn a_run_that_cannot_take_the_generation_slot_gives_the_install_lock_back() {
        let install = InstallState::default();
        let generation = GenerationState::default();
        let lab = LabState::default();
        generation.lock().unwrap().active_cancel = Some(Arc::new(AtomicBool::new(false)));
        assert!(begin_lab_run(&install, &generation, &lab).is_err());
        let lock = begin_install(&install).expect("nothing is left holding the install lock");
        finish_install(&install, &lock).unwrap();
    }

    #[test]
    fn a_run_cannot_start_while_a_model_install_is_in_progress() {
        let install = InstallState::default();
        let generation = GenerationState::default();
        let lab = LabState::default();
        let installing = begin_install(&install).unwrap();
        let refused = begin_lab_run(&install, &generation, &lab).unwrap_err();
        assert_eq!(refused.code, ProviderErrorCode::GenerationBusy);
        assert!(generation.lock().unwrap().active_cancel.is_none());
        finish_install(&install, &installing).unwrap();
        begin_lab_run(&install, &generation, &lab).unwrap();
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
    fn exit_stops_a_run_and_waits_for_it_to_end() {
        let generation = GenerationState::default();
        let lab = LabState::default();
        assert!(stop_lab_and_wait(&lab, Duration::from_millis(10)));

        let cancel = begin_lab_exclusive(&generation, &lab).unwrap();
        // The run's thread ends once it sees the cancel, as a real run does.
        let worker = {
            let (generation, lab, cancel) = (generation.clone(), lab.clone(), cancel.clone());
            std::thread::spawn(move || {
                while !cancel.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(5));
                }
                finish_lab(&generation, &lab, &cancel).unwrap();
            })
        };
        assert!(stop_lab_and_wait(&lab, Duration::from_secs(5)));
        worker.join().unwrap();
        assert!(cancel.load(Ordering::Acquire));

        // A run that doesn't end in time is reported, not waited on forever.
        let stuck = begin_lab_exclusive(&generation, &lab).unwrap();
        assert!(!stop_lab_and_wait(&lab, Duration::from_millis(60)));
        assert!(stuck.load(Ordering::Acquire));
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

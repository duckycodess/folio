//! Wiring from a [`ModelStore`] to the runner's providers, shared by the app's
//! commands and the headless measurement run so both exercise the same code.

use crate::contracts::{ModelDescriptor, ProviderErrorCode};
use crate::embeddings::{OrtE5Provider, DEFAULT_BATCH_SIZE, DEFAULT_MAX_TOKENS};
use crate::error::{CoreError, CoreResult, NativeProviderErrorError};
use crate::generation::{LabServerOptions, LlamaServerProvider};
use crate::lab::candidates::{self, candidate_store, is_candidate_id};
use crate::lab::host::{llama_server_devices, llama_server_version};
use crate::lab::record::{
    GpuOffload, ModelCatalog, ModelFileRef, ModelRef, RuntimeBackend, RuntimeDetail, RuntimeName,
};
use crate::lab::runner::{GeneratorFactory, GeneratorHandle};
use crate::models::ModelStore;
use std::path::PathBuf;

/// The pinned identity a record keeps for a model, with the catalog it came
/// from. A candidate's own license caveat travels with it.
pub fn model_ref_in(descriptor: &ModelDescriptor, catalog: ModelCatalog) -> ModelRef {
    let candidate = catalog == ModelCatalog::EvaluationCandidate;
    ModelRef {
        id: descriptor.id.clone(),
        role: descriptor.role.clone(),
        repo: descriptor.repo.clone(),
        revision: descriptor.revision.clone(),
        quantization: descriptor.quantization.clone(),
        files: descriptor
            .files
            .iter()
            .map(|file| ModelFileRef {
                path: file.path.clone(),
                sha256: file.sha256.clone(),
                bytes: file.bytes,
            })
            .collect(),
        catalog,
        evaluation_only: candidate,
        license: Some(descriptor.license.clone()),
        license_note: if candidate {
            candidates::license_note(&descriptor.id)
        } else {
            None
        },
    }
}

/// A product model's identity.
pub fn model_ref(descriptor: &ModelDescriptor) -> ModelRef {
    model_ref_in(descriptor, ModelCatalog::Product)
}

/// What a record says about the llama.cpp runtime: its exact version, the
/// device listing and the offload setting Folio asked for. `Disabled` is a
/// request (`--n-gpu-layers 0`); the backend the server actually reported is
/// attached per record from its own output, and a listing without a GPU is not
/// taken to mean the CPU was used.
pub fn llama_runtime_detail(
    store: &ModelStore,
    runtime_id: &str,
    executable: &std::path::Path,
    gpu_offload: GpuOffload,
) -> CoreResult<RuntimeDetail> {
    let descriptor = store.runtime(runtime_id)?;
    let (device_listing, unavailable_reason) = match llama_server_devices(executable) {
        Ok(listing) => (Some(listing), None),
        Err(error) => (None, Some(error.to_string())),
    };
    Ok(RuntimeDetail {
        name: RuntimeName::LlamaCpp,
        version: llama_server_version(executable)?,
        backend: Some(RuntimeBackend {
            runtime_id: runtime_id.to_string(),
            platform: descriptor.platform.clone(),
            device_listing,
            unavailable_reason,
            gpu_offload,
            observed_log_excerpt: None,
            gpu_layers_offloaded: None,
            layers_total: None,
        }),
    })
}

/// Opens one verified generation model at a time against the verified runtime,
/// from the product store or, for an evaluation candidate, its isolated store.
pub struct StoreGeneratorFactory {
    pub data_dir: PathBuf,
    pub executable: PathBuf,
    pub runtime: RuntimeDetail,
    pub threads: usize,
    /// Keep every layer on the CPU (`--n-gpu-layers 0`), the measurement target.
    pub cpu_only: bool,
    /// Where each server writes its output so the backend it chose can be read.
    pub log_dir: Option<PathBuf>,
}

impl GeneratorFactory for StoreGeneratorFactory {
    fn identify(&self, model_id: &str) -> CoreResult<(ModelRef, RuntimeDetail)> {
        let model = if is_candidate_id(model_id) {
            model_ref_in(
                &candidates::candidate_descriptor(model_id)?,
                ModelCatalog::EvaluationCandidate,
            )
        } else {
            model_ref(ModelStore::new(&self.data_dir)?.model(model_id)?)
        };
        Ok((model, self.runtime.clone()))
    }

    fn open(&self, model_id: &str) -> CoreResult<GeneratorHandle> {
        // Candidate ids never collide with product ids, so the id picks the store.
        let (store, catalog) = if is_candidate_id(model_id) {
            (
                candidate_store(&self.data_dir)?,
                ModelCatalog::EvaluationCandidate,
            )
        } else {
            (ModelStore::new(&self.data_dir)?, ModelCatalog::Product)
        };
        let verified = store.verified_model_file(model_id)?;
        let model = model_ref_in(&verified.descriptor, catalog);
        let provider =
            LlamaServerProvider::from_verified_model(&self.executable, verified, self.threads)?
                .with_lab_options(LabServerOptions {
                    gpu_layers: self.cpu_only.then_some(0),
                    device: self.cpu_only.then(|| "none".to_string()),
                    // Only `restart` may start a process, so a request can never
                    // run on a server the lab did not restart and measure.
                    no_implicit_start: true,
                    log_path: self
                        .log_dir
                        .as_ref()
                        .map(|dir| dir.join(format!("{model_id}.log"))),
                });
        Ok(GeneratorHandle {
            model,
            runtime: self.runtime.clone(),
            generator: Box::new(provider),
        })
    }
}

/// A fresh in-process embedding session over the model's verified files. It is
/// separate from the application's own embedding provider.
pub fn open_embedding(
    store: &ModelStore,
    descriptor: &ModelDescriptor,
) -> CoreResult<OrtE5Provider> {
    let missing = |what: &str| {
        CoreError::Provider(
            NativeProviderErrorError::new(
                ProviderErrorCode::ModelCorrupt,
                format!("The embedding model has no {what}."),
            )
            .with_detail(descriptor.id.clone()),
        )
    };
    let model_file = descriptor
        .files
        .iter()
        .find(|file| file.path.ends_with(".onnx"))
        .ok_or_else(|| missing("ONNX file"))?;
    let tokenizer_file = descriptor
        .files
        .iter()
        .find(|file| file.path.ends_with("tokenizer.json"))
        .ok_or_else(|| missing("tokenizer file"))?;
    let model_path = store.verified_file_path(&descriptor.id, &model_file.path)?;
    let tokenizer_path = store.verified_file_path(&descriptor.id, &tokenizer_file.path)?;
    OrtE5Provider::from_files(
        model_path,
        tokenizer_path,
        descriptor.id.clone(),
        descriptor.revision.clone(),
        descriptor.quantization.clone(),
        384,
        &model_file.sha256,
        &tokenizer_file.sha256,
        DEFAULT_MAX_TOKENS,
        DEFAULT_BATCH_SIZE,
        2,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::ModelRole;
    use crate::lab::record::RuntimeName;

    fn first_of(store: &ModelStore, role: ModelRole) -> ModelDescriptor {
        store
            .manifest()
            .models
            .iter()
            .find(|model| model.role == role)
            .unwrap()
            .clone()
    }

    #[test]
    fn a_model_ref_keeps_the_pinned_revision_quantization_and_files() {
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path()).unwrap();
        let descriptor = first_of(&store, ModelRole::Generation);
        let model = model_ref(&descriptor);
        assert_eq!(model.id, descriptor.id);
        assert_eq!(model.revision, descriptor.revision);
        assert_eq!(model.quantization, descriptor.quantization);
        assert_eq!(model.files.len(), descriptor.files.len());
        assert_eq!(model.files[0].sha256, descriptor.files[0].sha256);
        assert_eq!(model.files[0].bytes, descriptor.files[0].bytes);
    }

    #[test]
    fn a_candidate_ref_is_labelled_and_carries_its_license_caveat() {
        let descriptor =
            candidates::candidate_descriptor("gemma-sea-lion-v4.5-e2b-q4-k-m").unwrap();
        let model = model_ref_in(&descriptor, ModelCatalog::EvaluationCandidate);
        assert_eq!(model.catalog, ModelCatalog::EvaluationCandidate);
        assert!(model.evaluation_only);
        assert_eq!(model.license.as_deref(), Some("mit"));
        assert!(model.license_note.unwrap().contains("Unsettled"));

        let qwen = candidates::candidate_descriptor("qwen3.5-2b-q4-k-m").unwrap();
        let plain = model_ref_in(&qwen, ModelCatalog::EvaluationCandidate);
        assert!(plain.evaluation_only && plain.license_note.is_none());

        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path()).unwrap();
        let product = model_ref(&first_of(&store, ModelRole::Generation));
        assert_eq!(product.catalog, ModelCatalog::Product);
        assert!(!product.evaluation_only && product.license_note.is_none());
    }

    #[test]
    fn a_model_is_identified_without_being_opened_and_an_unknown_id_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let factory = StoreGeneratorFactory {
            data_dir: dir.path().to_path_buf(),
            executable: dir.path().join("llama-server"),
            runtime: RuntimeDetail {
                name: RuntimeName::LlamaCpp,
                version: "test".into(),
                backend: None,
            },
            threads: 1,
            cpu_only: true,
            log_dir: None,
        };
        let (candidate, runtime) = factory.identify("gemma-sea-lion-v4.5-e2b-q4-k-m").unwrap();
        assert!(candidate.evaluation_only);
        assert_eq!(candidate.files[0].bytes, 3_427_879_360);
        assert_eq!(runtime.version, "test");
        let (product, _) = factory.identify("qwen3-0.6b-q4-k-m").unwrap();
        assert!(!product.evaluation_only);
        assert!(factory.identify("no-such-model").is_err());
    }

    #[test]
    fn candidates_open_from_their_own_store_and_stay_closed_until_installed() {
        let dir = tempfile::tempdir().unwrap();
        let factory = StoreGeneratorFactory {
            data_dir: dir.path().to_path_buf(),
            executable: dir.path().join("llama-server"),
            runtime: RuntimeDetail {
                name: RuntimeName::LlamaCpp,
                version: "test".into(),
                backend: None,
            },
            threads: 1,
            cpu_only: true,
            log_dir: None,
        };
        match factory.open("qwen3.5-0.8b-q4-k-m") {
            Err(CoreError::Provider(failure)) => {
                assert_eq!(failure.code, ProviderErrorCode::ModelNotInstalled)
            }
            other => panic!("expected ModelNotInstalled, got {:?}", other.map(|_| ())),
        }
        // Nothing is created in the product models folder.
        assert!(!dir.path().join("models").exists());
    }

    #[test]
    fn models_that_are_not_installed_do_not_open() {
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path()).unwrap();
        let generation = first_of(&store, ModelRole::Generation);
        let factory = StoreGeneratorFactory {
            data_dir: dir.path().to_path_buf(),
            executable: dir.path().join("llama-server"),
            runtime: RuntimeDetail {
                name: RuntimeName::LlamaCpp,
                version: "test".into(),
                backend: None,
            },
            threads: 1,
            cpu_only: true,
            log_dir: None,
        };
        match factory.open(&generation.id) {
            Err(CoreError::Provider(failure)) => {
                assert_eq!(failure.code, ProviderErrorCode::ModelNotInstalled)
            }
            other => panic!("expected ModelNotInstalled, got {:?}", other.map(|_| ())),
        }

        let embedding = first_of(&store, ModelRole::Embedding);
        match open_embedding(&store, &embedding) {
            Err(CoreError::Provider(failure)) => {
                assert_eq!(failure.code, ProviderErrorCode::ModelNotInstalled)
            }
            other => panic!("expected ModelNotInstalled, got {:?}", other.map(|_| ())),
        }
    }
}

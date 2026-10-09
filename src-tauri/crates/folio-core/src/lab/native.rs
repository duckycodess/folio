//! Wiring from a [`ModelStore`] to the runner's providers, shared by the app's
//! commands and the headless measurement run so both exercise the same code.

use crate::contracts::{ModelDescriptor, ProviderErrorCode};
use crate::embeddings::{OrtE5Provider, DEFAULT_BATCH_SIZE, DEFAULT_MAX_TOKENS};
use crate::error::{CoreError, CoreResult, NativeProviderErrorError};
use crate::generation::LlamaServerProvider;
use crate::lab::host::{llama_server_devices, llama_server_version};
use crate::lab::record::{
    GpuOffload, ModelFileRef, ModelRef, RuntimeBackend, RuntimeDetail, RuntimeName,
};
use crate::lab::runner::{GeneratorFactory, GeneratorHandle};
use crate::models::ModelStore;
use std::path::PathBuf;

/// The pinned identity a record keeps for a model.
pub fn model_ref(descriptor: &ModelDescriptor) -> ModelRef {
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
    }
}

/// What a record says about the llama.cpp runtime: its exact version and the
/// backend facts the runtime reports. Folio passes no GPU-offload setting, so
/// `gpuOffload` is `runtimeDefault`; a device listing that shows no GPU is not
/// taken to mean the CPU was used.
pub fn llama_runtime_detail(
    store: &ModelStore,
    runtime_id: &str,
    executable: &std::path::Path,
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
            gpu_offload: GpuOffload::RuntimeDefault,
        }),
    })
}

/// Opens one verified generation model at a time against the verified runtime.
pub struct StoreGeneratorFactory {
    pub data_dir: PathBuf,
    pub executable: PathBuf,
    pub runtime: RuntimeDetail,
    pub threads: usize,
}

impl GeneratorFactory for StoreGeneratorFactory {
    fn open(&self, model_id: &str) -> CoreResult<GeneratorHandle> {
        let store = ModelStore::new(&self.data_dir)?;
        let verified = store.verified_model_file(model_id)?;
        let model = model_ref(&verified.descriptor);
        let provider =
            LlamaServerProvider::from_verified_model(&self.executable, verified, self.threads)?;
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

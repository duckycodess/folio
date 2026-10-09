//! Descriptor-derived resolution of the active persistent embedding space.
//!
//! The persistent index space is the one #27's `sync_embeddings` registers:
//! `embedding_sync::stored_index_space(provider.space())`. This module derives
//! the same value from the selected installed model's descriptor alone, so it
//! never loads ONNX Runtime, never takes the embedding mutex and does not
//! depend on whether a provider happens to be loaded.

use folio_core::contracts::{ModelDescriptor, ModelRole};
use folio_core::embeddings::{e5_inputs_from_descriptor, e5_provider_space};
use rusqlite::Connection;

use crate::db::NativeResult;
use crate::embedding_sync::stored_index_space;
use crate::index::{self, EmbeddingSpace, SelectedEmbeddingModel};

/// The #27 stored-chunk space for a descriptor, or `None` when the descriptor
/// is not a usable E5 embedding model.
pub(crate) fn persistent_space_for_descriptor(
    descriptor: &ModelDescriptor,
) -> NativeResult<Option<EmbeddingSpace>> {
    if !matches!(descriptor.role, ModelRole::Embedding) {
        return Ok(None);
    }
    let Ok(e5) = e5_inputs_from_descriptor(descriptor) else {
        return Ok(None);
    };
    stored_index_space(&e5_provider_space(&e5.inputs)).map(Some)
}

/// The fingerprint of the one space whose rows may be shown or used: the
/// selected, installed embedding model's stored space, and only when that
/// exact space has been registered by an embedding sync.
pub(crate) fn resolve_installed_descriptor(
    conn: &Connection,
    selected_installed: Option<&ModelDescriptor>,
) -> NativeResult<Option<String>> {
    let Some(descriptor) = selected_installed else {
        return Ok(None);
    };
    let Some(space) = persistent_space_for_descriptor(descriptor)? else {
        return Ok(None);
    };
    let selected = SelectedEmbeddingModel {
        model_id: descriptor.id.clone(),
        revision: descriptor.revision.clone(),
    };
    index::resolve_active_space(conn, Some(&selected), Some(&space))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::embedding_sync::stored_space_fingerprint;
    use folio_core::contracts::ModelFile;
    use folio_core::embeddings::{DEFAULT_MAX_TOKENS, E5_DIMENSIONS};
    use folio_core::retrieval::space_fingerprint;

    fn descriptor() -> ModelDescriptor {
        ModelDescriptor {
            id: "multilingual-e5-small".into(),
            role: ModelRole::Embedding,
            repo: "example/e5".into(),
            revision: "rev-1".into(),
            files: vec![
                ModelFile {
                    path: "onnx/model_quantized.onnx".into(),
                    sha256: "sha256:model".into(),
                    bytes: 1,
                    download_url: None,
                },
                ModelFile {
                    path: "tokenizer.json".into(),
                    sha256: "sha256:tokenizer".into(),
                    bytes: 1,
                    download_url: None,
                },
            ],
            quantization: "int8".into(),
            license: "mit".into(),
            runtime: "onnx".into(),
            optional_pack: false,
        }
    }

    #[test]
    fn descriptor_space_is_the_stored_chunk_space_not_the_snapshot_space() {
        let descriptor = descriptor();
        let e5 = e5_inputs_from_descriptor(&descriptor).unwrap();
        assert_eq!(e5.inputs.dimensions, E5_DIMENSIONS);
        assert_eq!(e5.inputs.max_tokens, DEFAULT_MAX_TOKENS);
        assert_eq!(e5.model_file, "onnx/model_quantized.onnx");
        assert_eq!(e5.tokenizer_file, "tokenizer.json");

        let provider_space = e5_provider_space(&e5.inputs);
        let derived = persistent_space_for_descriptor(&descriptor)
            .unwrap()
            .unwrap();
        let derived_fingerprint = crate::identity::embedding_space_fingerprint(
            &derived.model_id,
            &derived.revision,
            &derived.quantization,
            derived.dimensions,
            &derived.preprocessing_fingerprint,
        );
        assert_eq!(
            derived_fingerprint,
            stored_space_fingerprint(&provider_space).unwrap()
        );
        assert_ne!(derived_fingerprint, space_fingerprint(&provider_space));
        // Golden: any drift in a fingerprint input fails loudly. Changed by
        // #108's `title-path-chunk-v2` stored input (from `chunk-text-v1`).
        assert_eq!(
            derived.preprocessing_fingerprint,
            "998f0d1653c24a86982f29df9342d3592061d3d1468ef64de422504a3b0218f9"
        );
    }

    #[test]
    fn resolves_only_the_registered_stored_space_of_the_installed_selection() {
        let conn = db::open_in_memory().unwrap();
        let descriptor = descriptor();
        // Nothing registered: no active space, whatever the model.
        assert_eq!(
            resolve_installed_descriptor(&conn, Some(&descriptor)).unwrap(),
            None
        );
        assert_eq!(resolve_installed_descriptor(&conn, None).unwrap(), None);

        // The interim snapshot space registered through the webview-callable
        // command must not become the active space.
        let provider_space =
            e5_provider_space(&e5_inputs_from_descriptor(&descriptor).unwrap().inputs);
        let snapshot = EmbeddingSpace {
            model_id: provider_space.model_id.clone(),
            revision: provider_space.revision.clone(),
            quantization: provider_space.quantization.clone(),
            dimensions: provider_space.dimensions as u32,
            preprocessing_fingerprint: provider_space.preprocessing_fingerprint.clone(),
        };
        index::register_space(&conn, &snapshot).unwrap();
        assert_eq!(
            resolve_installed_descriptor(&conn, Some(&descriptor)).unwrap(),
            None
        );

        // #27's seam registers the stored space; that exact row resolves.
        let stored = stored_index_space(&provider_space).unwrap();
        let registered = index::register_space(&conn, &stored).unwrap();
        assert_eq!(
            resolve_installed_descriptor(&conn, Some(&descriptor)).unwrap(),
            Some(registered.clone())
        );

        // A new revision or changed file hash is a different space.
        let mut revised = descriptor.clone();
        revised.revision = "rev-2".into();
        assert_eq!(
            resolve_installed_descriptor(&conn, Some(&revised)).unwrap(),
            None
        );
        let mut rehashed = descriptor.clone();
        rehashed.files[0].sha256 = "sha256:other".into();
        assert_eq!(
            resolve_installed_descriptor(&conn, Some(&rehashed)).unwrap(),
            None
        );

        // Generation models and malformed descriptors never resolve.
        let mut generation = descriptor.clone();
        generation.role = ModelRole::Generation;
        assert_eq!(
            resolve_installed_descriptor(&conn, Some(&generation)).unwrap(),
            None
        );
        let mut no_tokenizer = descriptor;
        no_tokenizer.files.truncate(1);
        assert_eq!(
            resolve_installed_descriptor(&conn, Some(&no_tokenizer)).unwrap(),
            None
        );
    }
}

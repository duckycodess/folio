//! The evaluation-only model catalog.
//!
//! Candidates live in `model-evaluation-candidates.json`, apart from the product
//! manifest, and install under `<app data>/model-lab/candidates`. They are never
//! in `list_models`, never selectable through `select_model`, and never written
//! to the product selection settings. Promotion to a supported model is a
//! separate, user-confirmed change to the product manifest.

use crate::contracts::ModelDescriptor;
use crate::error::{CoreError, CoreResult};
use crate::models::{ModelManifest, ModelStore};
use serde_json::Value;
use std::path::Path;

const CANDIDATES_JSON: &str =
    include_str!("../../../../resources/model-evaluation-candidates.json");

pub const PURPOSE: &str = "evaluation-only";

/// A catalog is acceptable only if it says it is evaluation-only and marks
/// every entry, so an unmarked model can never slip in as a normal choice.
fn check(value: &Value) -> CoreResult<()> {
    if value["purpose"] != PURPOSE {
        return Err(CoreError::Message(
            "The candidate catalog is not marked evaluation-only.".into(),
        ));
    }
    let every_entry_is_marked = value["models"]
        .as_array()
        .is_some_and(|models| models.iter().all(|model| model["evaluationOnly"] == true));
    if !every_entry_is_marked {
        return Err(CoreError::Message(
            "Every candidate must be marked evaluationOnly.".into(),
        ));
    }
    Ok(())
}

fn parsed() -> CoreResult<Value> {
    let value: Value = serde_json::from_str(CANDIDATES_JSON)?;
    check(&value)?;
    Ok(value)
}

/// The candidate catalog as a manifest with no runtimes: the runtime still
/// comes from the product manifest.
pub fn candidate_manifest() -> CoreResult<ModelManifest> {
    let manifest: ModelManifest = serde_json::from_value(parsed()?)?;
    if !manifest.runtimes.is_empty() {
        return Err(CoreError::Message(
            "The candidate catalog does not carry runtimes.".into(),
        ));
    }
    Ok(manifest)
}

/// An isolated install store: files go under `<data dir>/model-lab/candidates`
/// and its settings are never read as the product selection.
pub fn candidate_store(data_dir: impl AsRef<Path>) -> CoreResult<ModelStore> {
    ModelStore::with_manifest(
        data_dir.as_ref().join("model-lab").join("candidates"),
        candidate_manifest()?,
    )
}

pub fn is_candidate_id(id: &str) -> bool {
    candidate_manifest()
        .map(|manifest| manifest.models.iter().any(|model| model.id == id))
        .unwrap_or(false)
}

/// The catalog's own license caveat for a candidate, if it has one.
pub fn license_note(id: &str) -> Option<String> {
    parsed().ok()?["models"]
        .as_array()?
        .iter()
        .find(|model| model["id"] == id)
        .and_then(|model| model["licenseNote"].as_str())
        .map(str::to_string)
}

pub fn candidate_descriptor(id: &str) -> CoreResult<ModelDescriptor> {
    candidate_manifest()?
        .models
        .into_iter()
        .find(|model| model.id == id)
        .ok_or_else(|| CoreError::Message(format!("{id} is not an evaluation candidate.")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::ModelRole;

    const PRODUCT_JSON: &str = include_str!("../../../../resources/model-manifest.json");

    /// id, repo, commit, file, bytes, SHA-256: the facts checked against the
    /// Hugging Face metadata API on 2026-10-10 and listed in docs/model-candidates.md.
    const PINS: &[(&str, &str, &str, &str, u64, &str)] = &[
        (
            "qwen3.5-0.8b-q4-k-m",
            "unsloth/Qwen3.5-0.8B-GGUF",
            "6ab461498e2023f6e3c1baea90a8f0fe38ab64d0",
            "Qwen3.5-0.8B-Q4_K_M.gguf",
            532_517_120,
            "bd258782e35f7f458f8aced1adc053e6e92e89bc735ba3be89d38a06121dc517",
        ),
        (
            "qwen3.5-2b-q4-k-m",
            "unsloth/Qwen3.5-2B-GGUF",
            "f6d5376be1edb4d416d56da11e5397a961aca8ae",
            "Qwen3.5-2B-Q4_K_M.gguf",
            1_280_835_840,
            "aaf42c8b7c3cab2bf3d69c355048d4a0ee9973d48f16c731c0520ee914699223",
        ),
        (
            "gemma-sea-lion-v4.5-e2b-q4-k-m",
            "aisingapore/Gemma-SEA-LION-v4.5-E2B-IT-GGUF",
            "3c0d3590d93771f3f3e8a879d812500651576171",
            "Gemma-SEA-LION-v4.5-E2B-IT-Q4_K_M.gguf",
            3_427_879_360,
            "624a18a8cfc3d8ee29752200f73dc02f5007eaac408d94309e8d8da27b2a7ed9",
        ),
        (
            "gemma-4-e2b-q4-k-m",
            "unsloth/gemma-4-E2B-it-GGUF",
            "0314792d7f1f7e229411f620751375812bb9faf2",
            "gemma-4-E2B-it-Q4_K_M.gguf",
            3_106_738_272,
            "740185b21d22ceb83a11c3aa62ad5842ef32c70f6096d756bbee85a1e4ec34b8",
        ),
        (
            "ministral-3-3b-q4-k-m",
            "mistralai/Ministral-3-3B-Instruct-2512-GGUF",
            "eb599d408350ea2bb60452cb86be7c7b2fc28227",
            "Ministral-3-3B-Instruct-2512-Q4_K_M.gguf",
            2_147_023_008,
            "9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8",
        ),
        (
            "lfm2.5-1.2b-q4-k-m",
            "LiquidAI/LFM2.5-1.2B-Instruct-GGUF",
            "8ed288026e23958ad9dfa92d53ed773a8eee7125",
            "LFM2.5-1.2B-Instruct-Q4_K_M.gguf",
            730_895_168,
            "b1b3de114215d9507409a662a501a631095a479a419584e8a2ded6304b19b4f5",
        ),
        (
            "granite-4.0-micro-q4-k-m",
            "ibm-granite/granite-4.0-micro-GGUF",
            "ec48475f0c811d812fbfb61975717a9c36eeb652",
            "granite-4.0-micro-Q4_K_M.gguf",
            2_099_502_528,
            "97c417dcc0534b0737c74016fb2af083cb17c3b51eaac621192d23961b7024eb",
        ),
    ];

    #[test]
    fn the_catalog_holds_exactly_the_verified_pins() {
        let manifest = candidate_manifest().unwrap();
        assert_eq!(manifest.models.len(), PINS.len());
        for (id, repo, commit, file, bytes, sha) in PINS {
            let model = manifest.models.iter().find(|m| m.id == *id).unwrap();
            assert_eq!(model.repo, *repo);
            assert_eq!(model.revision, *commit);
            assert_eq!(model.files.len(), 1, "only the text GGUF, no projector");
            assert_eq!(model.files[0].path, *file);
            assert_eq!(model.files[0].bytes, *bytes);
            assert_eq!(model.files[0].sha256, *sha);
            assert_eq!(
                model.files[0].download_url.as_deref(),
                Some(format!("https://huggingface.co/{repo}/resolve/{commit}/{file}").as_str())
            );
            assert_eq!(model.role, ModelRole::Generation);
            assert_eq!(model.quantization, "Q4_K_M");
            assert_eq!(model.runtime, "llama.cpp");
            assert!(model.optional_pack);
        }
    }

    #[test]
    fn every_entry_is_pinned_to_a_commit_and_a_hash_never_a_moving_ref() {
        for model in candidate_manifest().unwrap().models {
            assert_eq!(model.revision.len(), 40);
            assert!(model.revision.chars().all(|c| c.is_ascii_hexdigit()));
            for file in &model.files {
                assert_eq!(file.sha256.len(), 64);
                assert!(file.sha256.chars().all(|c| c.is_ascii_hexdigit()));
                assert!(file.bytes > 0);
                let url = file.download_url.as_deref().unwrap();
                assert!(
                    url.contains(&format!("/resolve/{}/", model.revision)),
                    "{url}"
                );
                assert!(!url.contains("/main/") && !url.contains("latest"), "{url}");
                assert!(!file.path.to_lowercase().contains("mmproj"));
            }
        }
    }

    #[test]
    fn candidates_are_never_in_the_product_manifest_and_the_product_manifest_has_no_marker() {
        let product = ModelStore::new(tempfile::tempdir().unwrap().path()).unwrap();
        for model in candidate_manifest().unwrap().models {
            assert!(
                product.manifest().models.iter().all(|p| p.id != model.id),
                "{} is also a product model",
                model.id
            );
            assert!(product.model(&model.id).is_err());
        }
        assert!(!PRODUCT_JSON.contains("evaluationOnly"));
        assert!(!PRODUCT_JSON.contains("evaluation-only"));
    }

    #[test]
    fn a_catalog_that_is_not_evaluation_only_is_refused() {
        let shipped: Value = serde_json::from_str(CANDIDATES_JSON).unwrap();
        check(&shipped).unwrap();

        let mut unmarked = shipped.clone();
        unmarked["models"][1]["evaluationOnly"] = Value::Bool(false);
        assert!(check(&unmarked).is_err());

        let mut missing = shipped.clone();
        missing["models"][0]
            .as_object_mut()
            .unwrap()
            .remove("evaluationOnly");
        assert!(check(&missing).is_err());

        let mut wrong_purpose = shipped;
        wrong_purpose["purpose"] = Value::from("product");
        assert!(check(&wrong_purpose).is_err());
    }

    #[test]
    fn candidates_install_in_their_own_folder_and_the_license_caveat_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let store = candidate_store(dir.path()).unwrap();
        assert_eq!(
            store.data_dir(),
            dir.path().join("model-lab").join("candidates")
        );
        assert!(is_candidate_id("qwen3.5-2b-q4-k-m"));
        assert!(!is_candidate_id("qwen3-0.6b-q4-k-m"));
        let note = license_note("gemma-sea-lion-v4.5-e2b-q4-k-m").unwrap();
        assert!(note.contains("Unsettled") && note.contains("Gemma 4"));
        assert_eq!(license_note("qwen3.5-0.8b-q4-k-m"), None);
        assert_eq!(
            candidate_descriptor("qwen3.5-0.8b-q4-k-m").unwrap().license,
            "apache-2.0"
        );
        assert!(candidate_descriptor("qwen3-0.6b-q4-k-m").is_err());
    }
}

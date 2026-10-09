//! Real Model Lab measurement, run by the manual `model-lab.yml` workflow.
//!
//! It installs the requested pinned models and the platform's llama.cpp runtime
//! through `ModelStore` (the same size- and SHA-256-verified path the app uses),
//! then runs the lab and writes every record to a JSON file. A model getting a
//! case wrong is a measurement, not a failure: this test asserts only that the
//! harness itself ran and recorded what it should.

use folio_core::error::CoreResult;
use folio_core::lab::candidates::{candidate_store, is_candidate_id};
use folio_core::lab::host::{host_info, onnxruntime_version};
use folio_core::lab::native::{
    llama_runtime_detail, model_ref, open_embedding, StoreGeneratorFactory,
};
use folio_core::lab::runner::{
    system_clock_ms, EmbeddingSubject, LabProgress, LabRunner, OsMemoryProbe, RunEnd,
};
use folio_core::lab::suite::{Corpus, Suite};
use folio_core::lab::workspace::LabWorkspaces;
use folio_core::lab::{GpuOffload, JsonFileSink, RuntimeDetail, RuntimeName};
use folio_core::models::ModelStore;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

fn required(name: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| panic!("{name} must be set"))
}

fn progress_printer(label: &'static str) -> impl FnMut(folio_core::models::DownloadProgress) {
    let mut last: Option<Instant> = None;
    move |progress| {
        let due = last.map_or(true, |at| at.elapsed() >= Duration::from_secs(10));
        if due || progress.received_bytes == progress.total_bytes {
            last = Some(Instant::now());
            println!(
                "{label} {} {}: {}/{} bytes",
                progress.item_id, progress.file, progress.received_bytes, progress.total_bytes
            );
        }
    }
}

#[test]
#[ignore = "downloads pinned models and runs real inference; run by the manual Model Lab workflow"]
fn model_lab_real_run() -> CoreResult<()> {
    let data_dir = PathBuf::from(required("FOLIO_LAB_DATA_DIR"));
    let output = PathBuf::from(required("FOLIO_LAB_OUTPUT"));
    let runtime_id = required("FOLIO_LAB_RUNTIME_ID");
    let embedding_id = required("FOLIO_LAB_EMBEDDING_MODEL");
    let generation_ids: Vec<String> = required("FOLIO_LAB_GENERATION_MODELS")
        .split(',')
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect();
    assert!(!generation_ids.is_empty(), "no generation model requested");

    let store = ModelStore::new(&data_dir)?;
    let cancel = AtomicBool::new(false);
    store.install_runtime(&runtime_id, &cancel, progress_printer("runtime"))?;
    let executable = store.verified_runtime_executable(&runtime_id)?;
    store.install_model(&embedding_id, &cancel, progress_printer("model"))?;
    for id in &generation_ids {
        // An evaluation candidate installs into its own isolated store.
        let target = if is_candidate_id(id) {
            candidate_store(&data_dir)?
        } else {
            ModelStore::new(&data_dir)?
        };
        target.install_model(id, &cancel, progress_printer("model"))?;
    }

    let embedding_descriptor = store.model(&embedding_id)?.clone();
    let provider = open_embedding(&store, &embedding_descriptor)?;
    let threads = std::thread::available_parallelism()
        .map(|count| count.get().saturating_sub(1).max(1))
        .unwrap_or(1);
    let run_id = format!("ci-{}", system_clock_ms());
    let workspaces = LabWorkspaces::new(&data_dir);
    let factory = StoreGeneratorFactory {
        data_dir: data_dir.clone(),
        executable: executable.clone(),
        runtime: llama_runtime_detail(&store, &runtime_id, &executable, GpuOffload::Disabled)?,
        threads,
        cpu_only: true,
        log_dir: Some(workspaces.logs_dir(&run_id)?),
    };
    let suite = Suite::embedded()?;
    let corpus = Corpus::embedded();
    let mut sink = JsonFileSink::new(&output);
    let end = LabRunner {
        run_id: run_id.clone(),
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
        generation_model_ids: generation_ids.clone(),
        factory: &factory,
        probe: &OsMemoryProbe,
        sink: &mut sink,
        cancel: &cancel,
        threads: threads as u32,
        host: host_info(),
        clock_ms: system_clock_ms,
        progress: Some(&|progress: &LabProgress| println!("lab {progress:?}")),
    }
    .run()?;

    assert_eq!(end, RunEnd::Completed);
    let export = sink.export();
    // Three retrieval cases for the embedding model, three generation cases for
    // each generation model, each measured twice (first request, then repeat).
    assert_eq!(export.records.len(), 2 * (3 + 3 * generation_ids.len()));
    for record in &export.records {
        record.validate()?;
    }
    println!(
        "Recorded {} measurements to {}",
        export.records.len(),
        output.display()
    );
    Ok(())
}

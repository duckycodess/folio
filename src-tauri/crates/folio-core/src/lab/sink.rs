use super::record::{BenchmarkRecord, RunSummary, SchemaVersion};
use crate::error::CoreResult;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Where the harness reports. A sink that fails stops the run: a measurement
/// that cannot be saved must not be reported as recorded.
pub trait LabSink {
    /// Called once per finished case, in run order.
    fn record(&mut self, record: &BenchmarkRecord) -> CoreResult<()>;
    /// Called when a run starts and whenever its status changes.
    fn run_status(&mut self, run: &RunSummary) -> CoreResult<()>;
}

/// Keeps everything in memory. Used by tests and by callers that export later.
#[derive(Debug, Default)]
pub struct MemorySink {
    pub records: Vec<BenchmarkRecord>,
    pub runs: Vec<RunSummary>,
}

impl LabSink for MemorySink {
    fn record(&mut self, record: &BenchmarkRecord) -> CoreResult<()> {
        record.validate()?;
        self.records.push(record.clone());
        Ok(())
    }

    fn run_status(&mut self, run: &RunSummary) -> CoreResult<()> {
        self.runs.push(run.clone());
        Ok(())
    }
}

/// The file a [`JsonFileSink`] writes, and what a CI artifact contains.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LabExport {
    pub schema_version: SchemaVersion,
    pub runs: Vec<RunSummary>,
    pub records: Vec<BenchmarkRecord>,
}

/// Writes everything reported so far to one JSON file after each report, so a
/// crash keeps the cases that finished. The file is replaced, not appended to.
pub struct JsonFileSink {
    path: PathBuf,
    export: LabExport,
}

impl JsonFileSink {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            export: LabExport {
                schema_version: SchemaVersion,
                runs: Vec::new(),
                records: Vec::new(),
            },
        }
    }

    pub fn export(&self) -> &LabExport {
        &self.export
    }

    fn flush(&self) -> CoreResult<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let partial = self.path.with_extension("json.partial");
        fs::write(&partial, serde_json::to_vec_pretty(&self.export)?)?;
        fs::rename(&partial, &self.path)?;
        Ok(())
    }
}

impl LabSink for JsonFileSink {
    fn record(&mut self, record: &BenchmarkRecord) -> CoreResult<()> {
        record.validate()?;
        self.export.records.push(record.clone());
        self.flush()
    }

    fn run_status(&mut self, run: &RunSummary) -> CoreResult<()> {
        match self
            .export
            .runs
            .iter_mut()
            .find(|existing| existing.run_id == run.run_id)
        {
            Some(existing) => *existing = run.clone(),
            None => self.export.runs.push(run.clone()),
        }
        self.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lab::record::{HostInfo, RunStatus, ServerSettings, StartupWarmup, SuiteRef};

    const GOLDEN: &str = include_str!("../../../../../fixtures/contracts/benchmark-record.json");

    fn record(id: &str) -> BenchmarkRecord {
        let mut record: BenchmarkRecord = serde_json::from_str(GOLDEN).unwrap();
        record.id = id.into();
        record
    }

    fn run(status: RunStatus) -> RunSummary {
        RunSummary {
            run_id: "run-1".into(),
            status,
            requested_model_ids: vec!["e".into(), "g".into()],
            suite: SuiteRef {
                id: "s".into(),
                sha256: "0".repeat(64),
                frozen: false,
            },
            corpus_sha256: "1".repeat(64),
            host: HostInfo {
                os: "o".into(),
                os_version: None,
                arch: "a".into(),
                cpu_brand: None,
                logical_cpus: 1,
                installed_ram_bytes: None,
            },
            server_settings: ServerSettings {
                startup_warmup: StartupWarmup::DefaultOn,
                cache_prompt: false,
            },
            started_at: 1,
            ended_at: None,
            index_build_ms: None,
            error: None,
            schema_version: SchemaVersion,
        }
    }

    #[test]
    fn the_json_file_holds_every_report_and_updates_a_run_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out").join("results.json");
        let mut sink = JsonFileSink::new(&path);
        sink.run_status(&run(RunStatus::Running)).unwrap();
        sink.record(&record("a")).unwrap();
        // The finished case is already on disk before the run ends.
        let midway: LabExport = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(midway.records.len(), 1);
        assert_eq!(midway.runs[0].status, RunStatus::Running);

        sink.record(&record("b")).unwrap();
        sink.run_status(&run(RunStatus::Completed)).unwrap();
        let done: LabExport = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(&done, sink.export());
        assert_eq!(done.records.len(), 2);
        assert_eq!(done.runs.len(), 1);
        assert_eq!(done.runs[0].status, RunStatus::Completed);
        assert!(!path.with_extension("json.partial").exists());
    }

    #[test]
    fn an_invalid_record_is_not_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("results.json");
        let mut sink = JsonFileSink::new(&path);
        let mut bad = record("a");
        bad.correctness = Some(true);
        assert!(sink.record(&bad).is_err());
        assert!(sink.export().records.is_empty());
        assert!(!path.exists());
    }
}

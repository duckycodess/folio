use super::record::{BenchmarkRecord, RunSummary};
use crate::error::CoreResult;

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

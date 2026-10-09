import { useId, useState } from "react";
import {
  budgetLabels,
  canReview,
  cpuLabel,
  durationLabel,
  memoryLabels,
  outcomeLabel,
  progressLabel,
  recordModelLabel,
  recordsByTask,
  requestLabel,
  runConditionRows,
  runLabel,
  runtimeLabel,
  RUN_STATUS_LABELS,
} from "../app/modelLab";
import { modelName, TASK_LABELS } from "../app/models";
import type { ModelLabController } from "../app/useModelLab";
import type { BenchmarkRecord } from "../domain/contracts";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { EmptyState } from "../ui/EmptyState";
import { Notice } from "../ui/Notice";
import { Panel } from "../ui/Panel";
import { Progress } from "../ui/Progress";
import { RecoveryNotice } from "../ui/RecoveryNotice";
import { EvaluationCandidates } from "./ModelLabCandidates";
import { ReviewDialog } from "./ModelLabReview";

/** Choose installed models and run the fixed tasks on them, one at a time. */
export function CompareModels({
  lab,
  productDownloading,
}: {
  lab: ModelLabController;
  productDownloading: boolean;
}) {
  const embeddingId = useId();
  const { choices, selection, running } = lab;

  return (
    <Panel title="Compare models">
      <p className="muted">
        Runs a small fixed set of tasks on a copy of sample files, one model at
        a time. Your own files aren't used or changed.
      </p>
      {lab.error && (
        <RecoveryNotice
          error={lab.error}
          actions={{ retry: lab.reload }}
          onDismiss={lab.dismiss}
        />
      )}
      {lab.ended && !running && (
        <Notice
          tone={lab.ended.step === "failed" ? "warning" : "info"}
          action={
            <button type="button" className="link-button" onClick={lab.dismiss}>
              Dismiss
            </button>
          }
        >
          {progressLabel(lab.ended, lab.models)}
          {lab.ended.error ? `: ${lab.ended.error}` : "."}
        </Notice>
      )}

      {lab.load === "desktopOnly" ? (
        <EmptyState title="Comparing models works in the desktop app">
          This preview has no measurements to show, and never shows sample
          numbers in their place.
        </EmptyState>
      ) : lab.load === "loading" ? (
        <Progress label="Reading installed models and recorded runs" />
      ) : lab.load === "failed" ? null : (
        <div className="lab-compare">
          <fieldset className="lab-choice" disabled={running !== null}>
            <legend className="lab-legend">
              <label htmlFor={embeddingId}>Search model</label>
            </legend>
            {choices.embedding.length ? (
              <select
                id={embeddingId}
                className="select"
                value={selection.embeddingModelId ?? ""}
                onChange={(event) => lab.setEmbedding(event.target.value)}
              >
                {choices.embedding.map((model) => (
                  <option key={model.id} value={model.id}>
                    {modelName(model)}
                    {model.selected ? " (in use)" : ""}
                  </option>
                ))}
              </select>
            ) : (
              <p className="muted">No search model is installed.</p>
            )}
          </fieldset>

          <fieldset className="lab-choice" disabled={running !== null}>
            <legend className="lab-legend">Writing models, in run order</legend>
            {choices.generation.length ? (
              <ul className="lab-models">
                {choices.generation.map((model) => {
                  const order = selection.generationModelIds.indexOf(model.id);
                  return (
                    <li key={model.id}>
                      <label className="lab-model">
                        <input
                          type="checkbox"
                          checked={order >= 0}
                          onChange={() => lab.toggleGeneration(model.id)}
                        />
                        <span className="lab-model-name">
                          <span>
                            {modelName(model)}
                            {model.selected ? " (in use)" : ""}
                          </span>
                          {model.evaluationOnly && (
                            <Badge>Evaluation only, not supported</Badge>
                          )}
                          {order >= 0 &&
                            selection.generationModelIds.length > 1 && (
                              <span className="muted tabular">
                                {order + 1}
                                {ordinal(order + 1)}
                              </span>
                            )}
                        </span>
                      </label>
                    </li>
                  );
                })}
              </ul>
            ) : (
              <p className="muted">No writing model is installed.</p>
            )}
          </fieldset>

          {running ? (
            <div className="model-progress">
              <Progress label={progressLabel(running.progress, lab.models)} />
              <Button
                onClick={lab.cancel}
                disabled={running.cancelling || running.runId === null}
              >
                {running.cancelling ? "Stopping…" : "Stop"}
              </Button>
            </div>
          ) : (
            <div className="lab-start">
              <Button
                variant="primary"
                onClick={lab.start}
                disabled={lab.blocked !== null}
              >
                Start comparison
              </Button>
              {lab.blocked && <span className="muted">{lab.blocked}</span>}
            </div>
          )}
          <EvaluationCandidates
            lab={lab}
            productDownloading={productDownloading}
          />
          <p className="muted">
            While it runs, summaries and Ask &amp; Act wait, and model downloads
            can't start.
          </p>
        </div>
      )}
    </Panel>
  );
}

function ordinal(n: number): string {
  return n === 1 ? "st" : n === 2 ? "nd" : n === 3 ? "rd" : "th";
}

function RecordsTable({
  records,
  onReview,
}: {
  records: BenchmarkRecord[];
  onReview: (record: BenchmarkRecord) => void;
}) {
  return (
    <div className="table-scroll">
      <table className="results-table">
        <thead>
          <tr>
            <th scope="col">Model</th>
            <th scope="col">Case</th>
            <th scope="col">Result</th>
            <th scope="col">Request</th>
            <th scope="col">Time</th>
            <th scope="col">Peak memory (process, not device)</th>
            <th scope="col">Runtime</th>
            <th scope="col">Budget</th>
          </tr>
        </thead>
        <tbody>
          {records.map((record) => (
            <tr key={record.id}>
              <td>
                {recordModelLabel(record)}
                <br />
                <span className="muted">{record.revision.slice(0, 12)}</span>
              </td>
              <td>{record.caseId}</td>
              <td>
                {outcomeLabel(record)}
                {record.retryNeeded && (
                  <>
                    <br />
                    <span className="muted">Worth running again</span>
                  </>
                )}
                {canReview(record) && (
                  <>
                    <br />
                    <button
                      type="button"
                      className="link-button"
                      onClick={() => onReview(record)}
                    >
                      Review
                      <span className="visually-hidden">
                        {" "}
                        {record.caseId}, {requestLabel(record).toLowerCase()}
                      </span>
                    </button>
                  </>
                )}
              </td>
              <td>{requestLabel(record)}</td>
              <td className="tabular">{durationLabel(record)}</td>
              <td>
                {memoryLabels(record).map((line) => (
                  <div key={line}>{line}</div>
                ))}
              </td>
              <td>
                {runtimeLabel(record)}
                <br />
                <span className="muted">{cpuLabel(record)}</span>
              </td>
              <td>
                {budgetLabels(record).map((line) => (
                  <div key={line}>{line}</div>
                ))}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/** Recorded runs and their per-task results, never combined into a score. */
export function RecordedResults({ lab }: { lab: ModelLabController }) {
  const runSelectId = useId();
  const [reviewing, setReviewing] = useState<string | null>(null);
  if (lab.load !== "ready") return null;
  const reviewed = lab.records.find((record) => record.id === reviewing);
  const run = lab.runs.find((candidate) => candidate.runId === lab.runId);

  return (
    <Panel title="Recorded results">
      <p className="muted">
        Each task is measured on its own, with the conditions it ran under.
        There's no overall score. One first request and one repeat per case is
        an early observation, not a stable speed.
      </p>
      {lab.runs.length === 0 ? (
        <EmptyState title="No comparisons recorded yet">
          Results for finding files, reading requests, summaries and edits
          appear here after a comparison runs on this computer.
        </EmptyState>
      ) : (
        <>
          <div className="filter lab-run">
            <label className="filter-label" htmlFor={runSelectId}>
              Run
            </label>
            <select
              id={runSelectId}
              className="select"
              value={lab.runId ?? ""}
              onChange={(event) => lab.chooseRun(event.target.value)}
            >
              {lab.runs.map((candidate) => (
                <option key={candidate.runId} value={candidate.runId}>
                  {runLabel(candidate)}
                </option>
              ))}
            </select>
          </div>
          {run?.error && (
            <Notice tone="warning">
              {RUN_STATUS_LABELS[run.status]}: {run.error}
            </Notice>
          )}
          {lab.recordsLoading ? (
            <Progress label="Reading the run's results" />
          ) : (
            <>
              {run && (
                <dl className="lab-conditions">
                  {runConditionRows(run).map((row) => (
                    <div key={row.label}>
                      <dt>{row.label}</dt>
                      <dd>{row.value}</dd>
                    </div>
                  ))}
                </dl>
              )}
              {recordsByTask(lab.records).map(({ task, records }) => (
                <section key={task} className="results-task">
                  <h3 className="graph-list-heading">{TASK_LABELS[task]}</h3>
                  {records.length ? (
                    <RecordsTable
                      records={records}
                      onReview={(record) => setReviewing(record.id)}
                    />
                  ) : (
                    <p className="muted">No results recorded for this task.</p>
                  )}
                </section>
              ))}
            </>
          )}
        </>
      )}
      {reviewed && (
        <ReviewDialog
          record={reviewed}
          lab={lab}
          onClose={() => setReviewing(null)}
        />
      )}
    </Panel>
  );
}

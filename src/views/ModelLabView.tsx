import { useState } from "react";
import {
  correctnessLabel,
  exactSize,
  modelName,
  progressPercent,
  ramLabel,
  resultsByTask,
  ROLE_LABELS,
  TASK_LABELS,
  totalDownloadBytes,
  type ModelRow,
} from "../app/models";
import { useModels, type ModelsController } from "../app/useModels";
import type { BenchmarkResult, ModelDescriptor } from "../domain/contracts";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { EmptyState } from "../ui/EmptyState";
import { Modal } from "../ui/Modal";
import { Notice } from "../ui/Notice";
import { Olio } from "../ui/Olio";
import { Panel } from "../ui/Panel";
import { Progress } from "../ui/Progress";
import { RecoveryNotice } from "../ui/RecoveryNotice";

function statusLabel(row: ModelRow): string {
  switch (row.state?.status) {
    case undefined:
      return "Checking…";
    case "installed":
      return "Installed";
    case "notInstalled":
      return "Not installed";
    case "downloading":
      return "Downloading";
    case "verifying":
      return "Checking files";
    case "corrupt":
      return "Damaged: download again";
  }
}

function ModelCard({
  row,
  models,
  onRemove,
}: {
  row: ModelRow;
  models: ModelsController;
  onRemove: (descriptor: ModelDescriptor) => void;
}) {
  const { descriptor, state, selected } = row;
  const installed = state?.status === "installed";
  const busy = models.installing?.modelId === descriptor.id;
  const locked = !!models.installing || !!models.saving;
  const total = totalDownloadBytes(descriptor, models.runtime, models.setup);
  const needsRuntime =
    descriptor.role === "generation" && models.runtime?.installed === false;
  const name = modelName(descriptor);

  return (
    <li className="model-card">
      <div className="model-card-head">
        <h3 className="model-name">{name}</h3>
        {selected && <Badge dot="var(--color-success-dot)">In use</Badge>}
        <Badge>{statusLabel(row)}</Badge>
        {descriptor.optionalPack && <Badge>Optional, larger</Badge>}
      </div>
      <dl className="model-facts">
        <dt>Revision</dt>
        <dd className="tabular" title={descriptor.revision}>
          {descriptor.revision.slice(0, 12)}
        </dd>
        <dt>Download</dt>
        <dd className="tabular">{exactSize(row.downloadBytes)}</dd>
        <dt>Runtime</dt>
        <dd>{descriptor.runtime}</dd>
        <dt>License</dt>
        <dd>{descriptor.license}</dd>
        <dt>Source</dt>
        <dd>{descriptor.repo}</dd>
      </dl>
      {needsRuntime && !installed && (
        <p className="muted">
          Also downloads the llama.cpp {models.runtime?.version} runtime for
          this computer
          {models.setup?.hostRuntimeBytes != null
            ? `: ${exactSize(models.setup.hostRuntimeBytes)}`
            : " (size not listed)"}
          .
        </p>
      )}
      {busy && models.installing ? (
        <div className="model-progress">
          <Progress
            label={
              models.installing.cancelling
                ? "Cancelling"
                : models.installing.step === "runtime"
                  ? "Downloading the llama.cpp runtime"
                  : `Downloading ${models.installing.progress?.file ?? name}`
            }
            value={progressPercent(models.installing.progress)}
          />
          <Button
            onClick={models.cancel}
            disabled={models.installing.cancelling}
          >
            Cancel download
          </Button>
        </div>
      ) : (
        <div className="form-actions">
          {!installed && (
            <Button
              variant="primary"
              disabled={locked || state === undefined}
              onClick={() => models.install(descriptor)}
            >
              {state?.status === "corrupt" ? "Download again" : "Download"} (
              {exactSize(total.bytes).split(" (")[0]}
              {total.runtimeUnknown ? " + runtime" : ""})
            </Button>
          )}
          {installed && !selected && (
            <Button
              variant="primary"
              disabled={locked}
              onClick={() => models.select(descriptor.role, descriptor.id)}
            >
              Use this model
            </Button>
          )}
          {installed && (
            <Button disabled={locked} onClick={() => onRemove(descriptor)}>
              Remove
            </Button>
          )}
        </div>
      )}
    </li>
  );
}

function ResultsTable({ results }: { results: BenchmarkResult[] }) {
  return (
    <div className="table-scroll">
      <table className="results-table">
        <thead>
          <tr>
            <th scope="col">Model</th>
            <th scope="col">Result</th>
            <th scope="col">Time</th>
            <th scope="col">Run</th>
            <th scope="col">Peak RAM</th>
            <th scope="col">Context</th>
            <th scope="col">Runtime and hardware</th>
          </tr>
        </thead>
        <tbody>
          {results.map((result) => (
            <tr
              key={`${result.caseId}-${result.modelId}-${result.revision}-${result.cold}`}
            >
              <td>
                {result.modelId} · {result.quantization}
                <br />
                <span className="muted">{result.revision.slice(0, 12)}</span>
              </td>
              <td>{correctnessLabel(result)}</td>
              <td className="tabular">
                {(result.taskDurationMs / 1000).toFixed(1)} s
              </td>
              <td>{result.cold ? "Cold start" : "Warm"}</td>
              <td>{ramLabel(result)}</td>
              <td className="tabular">{result.contextTokens} tokens</td>
              <td>
                {result.runtime}
                <br />
                <span className="muted">{result.hardware}</span>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/**
 * Model setup and Model Lab. Downloads use the pinned manifest's exact sizes
 * and hashes; results are shown per task with their recorded conditions and
 * never combined into one score.
 */
export function ModelLabView() {
  const models = useModels();
  const [removing, setRemoving] = useState<ModelDescriptor | null>(null);
  // Fixed-task measurements (#8) have no producer yet.
  const results: BenchmarkResult[] = [];

  return (
    <div className="view">
      <header className="page-header page-header-compact">
        <h1 className="page-title">Model Lab</h1>
        <p className="page-tagline">
          Set up and compare the local AI models Folio runs on this device.
        </p>
      </header>

      {models.error && (
        <RecoveryNotice
          error={models.error}
          actions={{ retry: models.reload }}
          onDismiss={models.dismiss}
        />
      )}
      {models.notice && (
        <Notice
          tone="info"
          action={
            <button
              type="button"
              className="link-button"
              onClick={models.dismiss}
            >
              Dismiss
            </button>
          }
        >
          {models.notice}
        </Notice>
      )}

      {models.load === "desktopOnly" ? (
        <Panel title="Local AI models">
          <EmptyState
            illustration={<Olio pose="sleeping" size={160} />}
            title="Model setup works in the desktop app"
          >
            This preview can't download or run models. Browsing, keyword search
            and reading files work without one.
          </EmptyState>
        </Panel>
      ) : models.load === "loading" ? (
        <Panel title="Local AI models">
          <Progress label="Checking which models are installed" />
        </Panel>
      ) : models.load === "failed" ? null : (
        models.groups.map((group) => (
          <Panel key={group.role} title={ROLE_LABELS[group.role].title}>
            <p className="muted">{ROLE_LABELS[group.role].purpose}</p>
            <ul className="model-list">
              {group.rows.map((row) => (
                <ModelCard
                  key={row.descriptor.id}
                  row={row}
                  models={models}
                  onRemove={setRemoving}
                />
              ))}
            </ul>
          </Panel>
        ))
      )}
      {models.load === "ready" && (
        <p className="muted">
          Sizes are the exact downloads from Folio's pinned list. The space a
          model takes once installed isn't measured.
        </p>
      )}

      <Panel title="Fixed-task results">
        <p className="muted">
          Each task is measured on its own, with the conditions it ran under.
          There's no overall score.
        </p>
        {results.length ? (
          resultsByTask(results).map(({ task, results: runs }) => (
            <section key={task} className="results-task">
              <h3 className="graph-list-heading">{TASK_LABELS[task]}</h3>
              {runs.length ? (
                <ResultsTable results={runs} />
              ) : (
                <p className="muted">No recorded runs.</p>
              )}
            </section>
          ))
        ) : (
          <EmptyState title="No results recorded yet">
            Measurements for finding files, reading requests, summaries and
            edits will appear here once they've been run on this device.
          </EmptyState>
        )}
      </Panel>

      {removing && (
        <Modal
          open
          title={`Remove ${modelName(removing)}?`}
          onClose={() => setRemoving(null)}
          footer={
            <>
              <Button onClick={() => setRemoving(null)}>Keep it</Button>
              <Button
                variant="primary"
                onClick={() => {
                  models.remove(removing);
                  setRemoving(null);
                }}
              >
                Remove
              </Button>
            </>
          }
        >
          <p>
            This deletes the downloaded model files from Folio's data folder.
            Your documents aren't touched, and you can download it again later.
          </p>
        </Modal>
      )}
    </div>
  );
}

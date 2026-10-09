import {
  exactSize,
  modelName,
  progressPercent,
  totalDownloadBytes,
  type ModelRow,
} from "../app/models";
import type { ModelsController } from "../app/useModels";
import type { ModelDescriptor } from "../domain/contracts";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { Progress } from "../ui/Progress";

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

/**
 * One model with its exact download size and revision. Download starts only
 * from its button, and is disabled when the disk is known to be too full.
 */
export function ModelCard({
  row,
  models,
  onRemove,
}: {
  row: ModelRow;
  models: ModelsController;
  /** Without it, an installed model has no Remove button (onboarding). */
  onRemove?: (descriptor: ModelDescriptor) => void;
}) {
  const { descriptor, state, selected } = row;
  const installed = state?.status === "installed";
  const busy = models.installing?.modelId === descriptor.id;
  const locked = !!models.installing || !!models.saving;
  const total = totalDownloadBytes(descriptor, models.runtime, models.setup);
  const needsRuntime =
    descriptor.role === "generation" && models.runtime?.installed === false;
  const name = modelName(descriptor);
  const free = models.setup?.availableDiskBytes ?? null;
  const tooBig = free !== null && free < total.bytes;

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
              disabled={locked || state === undefined || tooBig}
              aria-describedby={tooBig ? `${descriptor.id}-space` : undefined}
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
          {!installed && tooBig && free !== null && (
            <p id={`${descriptor.id}-space`} className="muted">
              Not enough free space: this needs{" "}
              {exactSize(total.bytes).split(" (")[0]}, and the disk Folio uses
              has {exactSize(free).split(" (")[0]} free.
            </p>
          )}
          {installed && onRemove && (
            <Button disabled={locked} onClick={() => onRemove(descriptor)}>
              Remove
            </Button>
          )}
        </div>
      )}
    </li>
  );
}

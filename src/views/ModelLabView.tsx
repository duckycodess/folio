import { useState } from "react";
import { modelName, ROLE_LABELS } from "../app/models";
import { useModelLab } from "../app/useModelLab";
import { useModels } from "../app/useModels";
import type { ModelDescriptor } from "../domain/contracts";
import { Button } from "../ui/Button";
import { EmptyState } from "../ui/EmptyState";
import { Modal } from "../ui/Modal";
import { Notice } from "../ui/Notice";
import { Panel } from "../ui/Panel";
import { Progress } from "../ui/Progress";
import { RecoveryNotice } from "../ui/RecoveryNotice";
import { ModelCard } from "./ModelCard";
import { CompareModels, RecordedResults } from "./ModelLabRuns";

/**
 * Model setup and Model Lab. Downloads use the pinned manifest's exact sizes
 * and hashes. Comparisons run through the native Model Lab (#8); their
 * results are shown per task with their recorded conditions and never
 * combined into one score.
 */
export function ModelLabView() {
  const models = useModels();
  const [removing, setRemoving] = useState<ModelDescriptor | null>(null);
  // Read the run's choices again when a model is installed, removed or chosen.
  const installed = models.groups
    .flatMap((group) => group.rows)
    .map((row) => `${row.descriptor.id}:${row.state?.status}:${row.selected}`)
    .join(",");
  const lab = useModelLab(installed);

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
          {/* No Olio here: the floating launcher is the view's one Olio (#66). */}
          <EmptyState title="Model setup works in the desktop app">
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

      <CompareModels lab={lab} />
      <RecordedResults lab={lab} />

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

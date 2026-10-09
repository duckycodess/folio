import { useState } from "react";
import { evaluationCandidates } from "../app/modelLab";
import { exactSize, modelName, progressPercent } from "../app/models";
import type { ModelLabController } from "../app/useModelLab";
import type { LabModel } from "../domain/contracts";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { Modal } from "../ui/Modal";
import { Progress } from "../ui/Progress";

const STATUS_LABELS: Record<LabModel["status"], string> = {
  notInstalled: "Not downloaded",
  downloading: "Downloading",
  verifying: "Checking",
  installed: "Downloaded",
  corrupt: "Damaged; download again",
};

/**
 * Evaluation-only models: downloaded only for comparing, into their own
 * folder, and never offered as the model Folio uses.
 */
export function EvaluationCandidates({
  lab,
  productDownloading,
}: {
  lab: ModelLabController;
  /** A product model download holds the same one-at-a-time install lock. */
  productDownloading: boolean;
}) {
  const [removing, setRemoving] = useState<LabModel | null>(null);
  const candidates = evaluationCandidates(lab.models);
  if (!candidates.length) return null;
  const { candidateInstall: installing } = lab;
  const locked =
    installing !== null ||
    lab.removingCandidate !== null ||
    lab.running !== null ||
    productDownloading;

  return (
    <section className="lab-candidates" aria-labelledby="lab-candidates-title">
      <h3 id="lab-candidates-title" className="graph-list-heading">
        Evaluation-only models
      </h3>
      <p className="muted">
        For comparing only. They aren't supported or recommended, and Folio
        never uses them for your files. Measuring one says nothing about whether
        it will be offered.
      </p>
      <ul className="lab-models">
        {candidates.map((model) => {
          const busy = installing?.modelId === model.id;
          const installed = model.status === "installed";
          return (
            <li key={model.id} className="lab-candidate">
              <div className="lab-model-name">
                <span>{modelName(model)}</span>
                <Badge>Evaluation only</Badge>
                <span className="muted">
                  {busy ? "Downloading" : STATUS_LABELS[model.status]}
                </span>
              </div>
              <p className="muted">
                {exactSize(model.modelFileBytes)} · license {model.license}
                {model.licenseNote ? ` (${model.licenseNote})` : ""}
              </p>
              {busy && installing ? (
                <div className="model-progress">
                  <Progress
                    label={
                      installing.cancelling
                        ? "Cancelling"
                        : `Downloading ${installing.progress?.file ?? modelName(model)}`
                    }
                    value={progressPercent(installing.progress)}
                  />
                  <Button
                    onClick={lab.cancelCandidate}
                    disabled={installing.cancelling}
                  >
                    Cancel download
                  </Button>
                </div>
              ) : (
                <div className="form-actions">
                  {installed ? (
                    <Button
                      disabled={locked}
                      onClick={() => setRemoving(model)}
                    >
                      {lab.removingCandidate === model.id
                        ? "Removing…"
                        : "Remove"}
                    </Button>
                  ) : (
                    <Button
                      disabled={locked}
                      onClick={() => lab.installCandidate(model.id)}
                    >
                      Download for comparing (
                      {exactSize(model.modelFileBytes).split(" (")[0]})
                    </Button>
                  )}
                </div>
              )}
            </li>
          );
        })}
      </ul>
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
                  lab.removeCandidate(removing.id);
                  setRemoving(null);
                }}
              >
                Remove
              </Button>
            </>
          }
        >
          <p>
            This deletes its downloaded files. Results already recorded for it
            are kept, and you can download it again later.
          </p>
        </Modal>
      )}
    </section>
  );
}

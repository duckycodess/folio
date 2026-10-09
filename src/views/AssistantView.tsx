import { useState } from "react";
import { simulatedFailure } from "../adapters/simulate";
import type { Drafts } from "../app/drafts";
import { folioError, type FolioError } from "../domain/errors";
import type { ViewId } from "../shell/navigation";
import { Button } from "../ui/Button";
import { Panel } from "../ui/Panel";
import { RecoveryNotice } from "../ui/RecoveryNotice";

export function AssistantView({
  drafts,
  onNavigate,
}: {
  drafts: Drafts;
  onNavigate: (view: ViewId) => void;
}) {
  const [failure, setFailure] = useState<FolioError | null>(null);
  const instruction = drafts.instruction;

  // There's no local model provider in this version, so every request ends
  // here; the instruction stays in the draft.
  function preview() {
    setFailure(
      simulatedFailure("assistant") ??
        folioError("modelNotInstalled", "No local model is installed."),
    );
  }

  return (
    <div className="view">
      <header className="page-header page-header-compact">
        <h1 className="page-title">Ask &amp; Act</h1>
        <p className="page-tagline">
          Describe a change. Folio finds the files, shows the exact change and
          waits for your approval.
        </p>
      </header>
      <Panel title="Instruction">
        <form
          className="assistant-form"
          onSubmit={(event) => {
            event.preventDefault();
            preview();
          }}
        >
          <label htmlFor="instruction" className="field-label">
            What should Folio do?
          </label>
          <textarea
            id="instruction"
            className="text-area"
            rows={4}
            value={instruction}
            placeholder="Hanapin yung project plan at palitan ang deadline…"
            onChange={(event) => {
              drafts.setInstruction(event.target.value);
              setFailure(null);
            }}
          />
          <div className="form-actions">
            <Button
              type="submit"
              variant="primary"
              disabled={!instruction.trim()}
            >
              Preview actions
            </Button>
          </div>
        </form>
        {failure && (
          <RecoveryNotice
            error={failure}
            actions={{
              openModelLab: () => onNavigate("modelLab"),
              retry: preview,
            }}
          />
        )}
      </Panel>
    </div>
  );
}

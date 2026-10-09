import { useState } from "react";
import type { ViewId } from "../shell/navigation";
import { Button } from "../ui/Button";
import { Notice } from "../ui/Notice";
import { Panel } from "../ui/Panel";

export function AssistantView({
  onNavigate,
}: {
  onNavigate: (view: ViewId) => void;
}) {
  const [instruction, setInstruction] = useState("");
  const [checked, setChecked] = useState(false);

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
            setChecked(true);
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
              setInstruction(event.target.value);
              setChecked(false);
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
        {checked && (
          <Notice
            tone="warning"
            action={
              <Button variant="ghost" onClick={() => onNavigate("modelLab")}>
                Open Model Lab
              </Button>
            }
          >
            Ask &amp; Act needs a local AI model, which isn't set up yet. No
            files were changed, and your instruction is kept.
          </Notice>
        )}
      </Panel>
    </div>
  );
}

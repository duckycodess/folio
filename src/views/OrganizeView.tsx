import { ArrowRight, Folders } from "lucide-react";
import { useState } from "react";
import type { WorkspaceState } from "../app/useWorkspace";
import { Button } from "../ui/Button";
import { EmptyState } from "../ui/EmptyState";
import { Modal } from "../ui/Modal";
import { Panel } from "../ui/Panel";
import { FileList } from "./FileList";

export function OrganizeView({ workspace }: { workspace: WorkspaceState }) {
  const selected = workspace.selected;
  const [name, setName] = useState("");
  const [previewOpen, setPreviewOpen] = useState(false);
  const trimmed = name.trim();
  const invalid = /[\\/]/.test(trimmed);

  return (
    <div className="view">
      <header className="page-header page-header-compact">
        <h1 className="page-title">Organize</h1>
        <p className="page-tagline">
          Group and rename files. Nothing changes on disk without your approval.
        </p>
      </header>

      <Panel title="Collections">
        <EmptyState icon={<Folders size={24} />} title="No collections yet">
          Collections group related files without moving or copying them.
          Creating collections isn't available in this version yet.
        </EmptyState>
      </Panel>

      <Panel title="Rename a file">
        {selected ? (
          <form
            className="rename-form"
            onSubmit={(event) => {
              event.preventDefault();
              if (trimmed && !invalid) setPreviewOpen(true);
            }}
          >
            <label htmlFor="rename-input" className="field-label">
              New name for {selected.name}
            </label>
            <div className="field-row">
              <input
                id="rename-input"
                className="text-input"
                value={name}
                placeholder={selected.name}
                aria-describedby="rename-help"
                aria-invalid={invalid || undefined}
                onChange={(event) => setName(event.target.value)}
              />
              <Button type="submit" disabled={!trimmed || invalid}>
                Preview rename
              </Button>
            </div>
            <p id="rename-help" className="field-help">
              {invalid
                ? "A file name can't contain / or \\."
                : "You'll see the exact change before anything happens."}
            </p>
          </form>
        ) : (
          <>
            <p className="muted">Choose a file to rename.</p>
            <FileList
              label="Files to rename"
              documents={workspace.documents}
              selectedId={undefined}
              onSelect={workspace.selectDocument}
            />
          </>
        )}
      </Panel>

      {selected && (
        <Modal
          open={previewOpen}
          title="Rename preview"
          onClose={() => setPreviewOpen(false)}
          footer={
            <Button onClick={() => setPreviewOpen(false)}>Close preview</Button>
          }
        >
          <div className="rename-preview">
            <span className="rename-from">{selected.name}</span>
            <ArrowRight size={18} aria-label="becomes" />
            <strong className="rename-to">{trimmed}</strong>
          </div>
          <p className="muted">
            Preview only — no file was changed. Applying renames isn't available
            in this version yet.
          </p>
        </Modal>
      )}
    </div>
  );
}

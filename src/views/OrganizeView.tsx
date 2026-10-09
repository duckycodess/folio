import { ArrowRight } from "lucide-react";
import { useState } from "react";
import type { WorkspaceState } from "../app/useWorkspace";
import type { DocumentRecord } from "../domain/contracts";
import { Button } from "../ui/Button";
import { EmptyState } from "../ui/EmptyState";
import { Modal } from "../ui/Modal";
import { Olio } from "../ui/Olio";
import { Panel } from "../ui/Panel";
import { listedSelection } from "../shell/reader";
import { FileList } from "./FileList";

export function OrganizeView({ workspace }: { workspace: WorkspaceState }) {
  // Like the reader, ignore a chosen file that the current search leaves out,
  // and offer only files the search includes.
  const selected = listedSelection(workspace.selected, workspace.results);
  const choices = workspace.results.map((result) => result.document);

  return (
    <div className="view">
      <header className="page-header page-header-compact">
        <h1 className="page-title">Organize</h1>
        <p className="page-tagline">
          Group and rename files. Nothing changes on disk without your approval.
        </p>
      </header>

      <Panel title="Collections">
        <EmptyState
          illustration={<Olio pose="organizing" size={96} />}
          title="No collections yet"
        >
          Collections group related files without moving or copying them.
          Creating collections isn't available in this version yet.
        </EmptyState>
      </Panel>

      <Panel title="Rename a file">
        {selected ? (
          // Keyed by file, so a typed name never carries over to another file.
          <RenameForm
            key={selected.id}
            document={selected}
            onChangeFile={() => {
              workspace.clearSelection();
              // The form is replaced by the file list; keep focus in it.
              requestAnimationFrame(() =>
                document
                  .querySelector<HTMLElement>(
                    '[aria-label="Files to rename"] [tabindex="0"]',
                  )
                  ?.focus(),
              );
            }}
          />
        ) : (
          <>
            <p className="muted">Choose a file to rename.</p>
            <FileList
              label="Files to rename"
              documents={choices}
              selectedId={undefined}
              onSelect={workspace.selectDocument}
            />
          </>
        )}
      </Panel>
    </div>
  );
}

function RenameForm({
  document,
  onChangeFile,
}: {
  document: DocumentRecord;
  onChangeFile: () => void;
}) {
  const [name, setName] = useState("");
  const [previewOpen, setPreviewOpen] = useState(false);
  const trimmed = name.trim();
  const invalid = /[\\/]/.test(trimmed);

  return (
    <>
      <form
        className="rename-form"
        onSubmit={(event) => {
          event.preventDefault();
          if (trimmed && !invalid) setPreviewOpen(true);
        }}
      >
        <label htmlFor="rename-input" className="field-label">
          New name for {document.name}
        </label>
        <div className="field-row">
          <input
            id="rename-input"
            className="text-input"
            value={name}
            placeholder={document.name}
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
        <div className="form-actions">
          <Button variant="ghost" onClick={onChangeFile}>
            Choose a different file
          </Button>
        </div>
      </form>

      <Modal
        open={previewOpen}
        title="Rename preview"
        onClose={() => setPreviewOpen(false)}
        footer={
          <Button onClick={() => setPreviewOpen(false)}>Close preview</Button>
        }
      >
        <div className="rename-preview">
          <span className="rename-from">{document.name}</span>
          <ArrowRight size={18} aria-label="becomes" />
          <strong className="rename-to">{trimmed}</strong>
        </div>
        <p className="muted">
          Preview only — no file was changed. Applying renames isn't available
          in this version yet.
        </p>
      </Modal>
    </>
  );
}

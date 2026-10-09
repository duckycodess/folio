import { ArrowRight } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { simulatedFailure } from "../adapters/simulate";
import type { Drafts } from "../app/drafts";
import type { OrganizeController } from "../app/useOrganize";
import type { WorkspaceState } from "../app/useWorkspace";
import type { DocumentRecord } from "../domain/contracts";
import type { FolioError } from "../domain/errors";
import { Button } from "../ui/Button";
import { EmptyState } from "../ui/EmptyState";
import { Modal } from "../ui/Modal";
import { Olio } from "../ui/Olio";
import { Panel } from "../ui/Panel";
import { RecoveryNotice } from "../ui/RecoveryNotice";
import { listedSelection } from "../shell/reader";
import { FileList } from "./FileList";
import { OrganizeFlowPanel } from "./OrganizeFlowPanel";

export function OrganizeView({
  workspace,
  drafts,
  organize,
}: {
  workspace: WorkspaceState;
  drafts: Drafts;
  organize: OrganizeController;
}) {
  const { plan, report } = organize.state;
  // A name typed for a file is done with once that rename is saved.
  useEffect(() => {
    if (!plan || !report) return;
    for (const outcome of report.batch.outcomes) {
      const operation = plan.operations[outcome.operationIndex];
      if (outcome.status === "succeeded" && operation?.kind === "rename")
        drafts.clearRenameName(operation.documentId);
    }
    // Only a new report clears drafts.
  }, [report]);

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

      <OrganizeFlowPanel workspace={workspace} organize={organize} />

      <Panel title="Collections">
        <EmptyState
          illustration={<Olio pose="organizing" size={96} />}
          title="No collections yet"
        >
          Collections are virtual: they group related files without moving or
          copying them. Creating collections isn't available in this version
          yet.
        </EmptyState>
      </Panel>

      <Panel title="Rename a file">
        {selected ? (
          // Keyed by file, so a typed name never carries over to another file.
          <RenameForm
            key={selected.id}
            document={selected}
            name={drafts.renameName(selected.id)}
            onNameChange={(name) => drafts.setRenameName(selected.id, name)}
            onPreview={
              workspace.source === "folder"
                ? (name) => organize.previewRename(selected, name)
                : undefined
            }
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
  name,
  onNameChange,
  onChangeFile,
  onPreview,
}: {
  document: DocumentRecord;
  /** Kept outside the form, so it survives errors and switching views. */
  name: string;
  onNameChange: (name: string) => void;
  onChangeFile: () => void;
  /** With a folder open, asks the native core for an exact plan. */
  onPreview?: (name: string) => void;
}) {
  const [previewOpen, setPreviewOpen] = useState(false);
  const [failure, setFailure] = useState<FolioError | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const trimmed = name.trim();
  const invalid = /[\\/]/.test(trimmed);

  function preview() {
    if (!trimmed || invalid) return;
    // In practice mode, the preview step fails the way a refused change would.
    const simulated = simulatedFailure("changes");
    setFailure(simulated ?? null);
    if (simulated) return;
    if (onPreview) onPreview(trimmed);
    else setPreviewOpen(true);
  }

  return (
    <>
      <form
        className="rename-form"
        onSubmit={(event) => {
          event.preventDefault();
          preview();
        }}
      >
        <label htmlFor="rename-input" className="field-label">
          New name for {document.name}
        </label>
        <div className="field-row">
          <input
            ref={input}
            id="rename-input"
            className="text-input"
            value={name}
            placeholder={document.name}
            aria-describedby="rename-help"
            aria-invalid={invalid || undefined}
            onChange={(event) => {
              onNameChange(event.target.value);
              setFailure(null);
            }}
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
        {failure && (
          <RecoveryNotice
            error={failure}
            actions={{
              previewAgain: preview,
              retry: preview,
              chooseAnotherName: () => input.current?.select(),
            }}
          />
        )}
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
          Preview only — no file was changed. Renaming works on a folder you add
          in the desktop app; sample files can't be changed.
        </p>
      </Modal>
    </>
  );
}

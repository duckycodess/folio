import { useMemo, useState } from "react";
import { moveFolders } from "../../app/fileActions";
import { moveOperation } from "../../app/useOrganize";
import { usePlanAction } from "../../app/usePlanAction";
import type { WorkspaceState } from "../../app/useWorkspace";
import type { DocumentRecord } from "../../domain/contracts";
import { Button } from "../../ui/Button";
import { PlanReview } from "../PlanReview";
import { ActionDialog } from "./ActionDialog";

/** Display name for a workspace folder; `""` is the folder the user added. */
function folderLabel(folder: string): string {
  return folder ? folder.split("/").join(" / ") : "Top folder";
}

/**
 * Move a file to another folder Folio already lists. A move keeps the file's
 * name and never creates a folder or replaces a file.
 */
export function MoveDialog({
  open,
  document,
  workspace,
  onClose,
  onFilesChanged,
}: {
  open: boolean;
  document: DocumentRecord;
  workspace: WorkspaceState;
  onClose: () => void;
  onFilesChanged?: () => void;
}) {
  const action = usePlanAction(workspace, onFilesChanged);
  const folders = useMemo(
    () => moveFolders(workspace.documents, document),
    [workspace.documents, document.relativePath],
  );
  const [chosen, setChosen] = useState<string | null>(null);
  const target = folders.find(
    (item) => item.folder === chosen && !item.blocked,
  );

  function preview() {
    if (!target) return;
    action.prepareFor(document, (read) => [moveOperation(read, target.folder)]);
  }

  return (
    <ActionDialog
      open={open}
      title={`Move ${document.name}`}
      workspace={workspace}
      document={document}
      action={action}
      onClose={onClose}
    >
      {action.state.stage === "idle" && (
        <form
          className="flow-step"
          onSubmit={(event) => {
            event.preventDefault();
            preview();
          }}
        >
          <fieldset className="move-folders">
            <legend className="field-label">
              Move {document.relativePath} to
            </legend>
            {folders.length ? (
              folders.map((item) => (
                <label key={item.folder} className="move-folder">
                  <input
                    type="radio"
                    name="move-folder"
                    value={item.folder}
                    checked={chosen === item.folder}
                    disabled={item.blocked !== undefined}
                    onChange={() => {
                      setChosen(item.folder);
                      action.dismissError();
                    }}
                  />
                  <span>
                    <span className="plan-path">
                      {folderLabel(item.folder)}
                    </span>
                    {item.blocked && (
                      <span className="plan-reason">{item.blocked}</span>
                    )}
                  </span>
                </label>
              ))
            ) : (
              <p className="muted">
                There's no other folder to move it to. Folio moves files only
                into folders that already exist.
              </p>
            )}
          </fieldset>
          <p className="field-help">
            Folio keeps the file's name and only lists folders that already
            exist. You'll see the exact change before anything happens.
          </p>
          <div className="form-actions">
            <Button type="submit" variant="primary" disabled={!target}>
              Preview move
            </Button>
            <Button variant="ghost" onClick={onClose}>
              Cancel
            </Button>
          </div>
        </form>
      )}
      <PlanReview
        action={action}
        inModal
        approveLabel="Approve and move"
        backLabel="Choose another folder"
        onBack={action.reset}
        onDone={onClose}
      />
    </ActionDialog>
  );
}

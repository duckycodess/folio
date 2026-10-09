import { useEffect, useId, useRef } from "react";
import type { Drafts } from "../../app/drafts";
import { renameProblem } from "../../app/fileActions";
import { renameOperation } from "../../app/useOrganize";
import { usePlanAction } from "../../app/usePlanAction";
import type { WorkspaceState } from "../../app/useWorkspace";
import type { DocumentRecord } from "../../domain/contracts";
import { Button } from "../../ui/Button";
import { PlanReview } from "../PlanReview";
import { ActionDialog } from "./ActionDialog";

/**
 * Rename a file in place, through the same native plan as Organize. The typed
 * name is a draft kept per file, so it survives errors and closing the dialog.
 */
export function RenameDialog({
  open,
  document,
  workspace,
  drafts,
  onClose,
  onFilesChanged,
}: {
  open: boolean;
  document: DocumentRecord;
  workspace: WorkspaceState;
  drafts: Drafts;
  onClose: () => void;
  onFilesChanged?: () => void;
}) {
  const action = usePlanAction(workspace, onFilesChanged);
  const input = useRef<HTMLInputElement>(null);
  const fieldId = useId();
  const name = drafts.renameName(document.id);
  const problem = name ? renameProblem(document.relativePath, name) : null;
  const { plan, report } = action.state;

  // A name is done with once that rename is saved.
  useEffect(() => {
    if (!plan || !report) return;
    for (const outcome of report.batch.outcomes) {
      const operation = plan.operations[outcome.operationIndex];
      if (outcome.status === "succeeded" && operation?.kind === "rename")
        drafts.clearRenameName(operation.documentId);
    }
  }, [report]);

  function preview() {
    if (!name.trim() || problem) return;
    action.prepareFor(document, (read) => [renameOperation(read, name.trim())]);
  }

  return (
    <ActionDialog
      open={open}
      title={`Rename ${document.name}`}
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
          <label htmlFor={fieldId} className="field-label">
            New name for {document.name}
          </label>
          <input
            ref={input}
            id={fieldId}
            className="text-input"
            value={name}
            placeholder={document.name}
            aria-describedby={`${fieldId}-help`}
            aria-invalid={problem ? true : undefined}
            onChange={(event) => {
              drafts.setRenameName(document.id, event.target.value);
              action.dismissError();
            }}
          />
          <p id={`${fieldId}-help`} className="field-help">
            {problem ??
              "The file stays in its folder. You'll see the exact change before anything happens."}
          </p>
          <div className="form-actions">
            <Button
              type="submit"
              variant="primary"
              disabled={!name.trim() || problem !== null}
            >
              Preview rename
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
        approveLabel="Approve and rename"
        backLabel="Change the name"
        onBack={() => {
          action.reset();
          requestAnimationFrame(() => input.current?.select());
        }}
        onDone={onClose}
      />
    </ActionDialog>
  );
}

import type { ReactNode } from "react";
import { fileActionAvailability } from "../../app/fileActions";
import { planActionBusy } from "../../app/planAction";
import type { PlanActionController } from "../../app/usePlanAction";
import type { WorkspaceState } from "../../app/useWorkspace";
import type { DocumentRecord } from "../../domain/contracts";
import { Button } from "../../ui/Button";
import { Modal } from "../../ui/Modal";

/**
 * The frame every file action dialog shares. It says why a change isn't
 * possible (browser preview, sample files, PDFs) instead of offering one, and
 * can't be closed while a change is being saved or undone.
 *
 * Keep the dialog mounted while it's closed: its draft and any result live in
 * it, so reopening it shows them again.
 */
export function ActionDialog({
  open,
  title,
  workspace,
  document,
  action,
  onClose,
  children,
}: {
  open: boolean;
  title: string;
  workspace: WorkspaceState;
  document: DocumentRecord;
  action: PlanActionController;
  onClose: () => void;
  children: ReactNode;
}) {
  const availability = fileActionAvailability(workspace, document);
  const busy = planActionBusy(action.state.stage);

  function close() {
    // An unapproved preview is dropped; nothing was written. A finished
    // result has been seen. The draft stays with the dialog. If the browser
    // closes the dialog mid-save anyway, the result is kept for reopening.
    if (!busy && (action.state.stage !== "idle" || action.state.error))
      action.reset();
    onClose();
  }

  return (
    <Modal
      open={open}
      title={title}
      onClose={close}
      className="modal-wide"
      dismissible={!busy}
    >
      {availability.available ? (
        children
      ) : (
        <>
          <p>{availability.reason}</p>
          <div className="form-actions">
            <Button onClick={close}>Close</Button>
          </div>
        </>
      )}
    </Modal>
  );
}

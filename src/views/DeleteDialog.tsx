import { useEffect, useRef } from "react";
import { readNativeDocument } from "../adapters/workspace";
import { deleteOperation } from "../app/graphActions";
import type { OrganizeController } from "../app/useOrganize";
import type { WorkspaceState } from "../app/useWorkspace";
import type { DocumentRecord } from "../domain/contracts";
import { folioError } from "../domain/errors";
import { Modal } from "../ui/Modal";
import { Progress } from "../ui/Progress";
import { RecoveryNotice } from "../ui/RecoveryNotice";
import { PreviewStep, ResultStep } from "./OrganizeFlowPanel";
import { ImpactList } from "./PlanReview";

/**
 * Delete one file (#45, using #44's native delete): the exact preview first,
 * with the links that will stop working, identical copies and anything the
 * local AI related to it. Cancel is the default; nothing is deleted until the
 * user approves, and the result offers Undo. Uses the same plan flow as
 * Organize, Rename and Move.
 */
export function DeleteDialog({
  document,
  workspace,
  action,
  onClose,
}: {
  document: DocumentRecord;
  workspace: WorkspaceState;
  action: OrganizeController;
  /** `deleted` is true when the file was deleted and not undone. */
  onClose: (deleted: boolean) => void;
}) {
  const { state } = action;
  const heading = useRef<HTMLHeadingElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  const changeable = workspace.source === "folder" && workspace.nativeAvailable;

  function prepare() {
    action.previewFrom(async (folderId) => {
      // The plan pins the exact revision; read it if the listing didn't.
      const read = document.contentHash
        ? document
        : await readNativeDocument(folderId, document);
      if (!read.contentHash)
        throw folioError("internal", "Folio couldn't read this file's hash.");
      return [deleteOperation({ ...read, contentHash: read.contentHash })];
    });
  }

  useEffect(() => {
    if (changeable) prepare();
    // Prepared once, when the dialog opens.
  }, []);

  // Cancel is the default in the preview; the result's heading is announced.
  // Leaving the preview without a result (Cancel) closes the dialog.
  const lastStage = useRef(state.stage);
  useEffect(() => {
    const previous = lastStage.current;
    lastStage.current = state.stage;
    if (state.stage === "preview") cancel.current?.focus();
    else if (state.stage === "result") heading.current?.focus();
    else if (previous === "preview" && state.stage !== "applying") close();
  }, [state.stage]);

  function close() {
    const deleted =
      state.report?.batch.outcomes.some(
        (outcome) => outcome.status === "succeeded",
      ) === true && !action.undo.report?.undoneEntryIds.length;
    action.done();
    onClose(deleted);
  }

  return (
    <Modal
      open
      title={`Delete ${document.name}`}
      // Open until an apply finishes, so its result and Undo aren't lost.
      dismissible={state.stage !== "applying"}
      onClose={close}
    >
      {!changeable ? (
        <p>
          Deleting works on a folder you add in the desktop app. Sample files
          can't be changed, so nothing was deleted.
        </p>
      ) : state.stage === "result" ? (
        <>
          <ResultStep organize={action} heading={heading} onDone={close} />
          <p className="muted">
            Undo is here, and in Activity for as long as Folio keeps the file's
            contents.
          </p>
        </>
      ) : state.stage === "preview" || state.stage === "applying" ? (
        <>
          <p>
            Folio keeps this file's exact contents, so you can undo the deletion
            afterwards.
          </p>
          <PreviewStep
            organize={action}
            heading={heading}
            cancelLabel="Cancel"
            cancelRef={cancel}
            details={
              state.plan ? (
                <ImpactList impacts={state.plan.impacts} deletion />
              ) : null
            }
          />
        </>
      ) : state.error ? (
        <RecoveryNotice
          error={state.error}
          actions={{ retry: prepare, previewAgain: prepare }}
          onDismiss={close}
        />
      ) : (
        <Progress label="Preparing the exact preview" />
      )}
    </Modal>
  );
}

import { useEffect, useRef } from "react";
import { readNativeDocument } from "../adapters/workspace";
import { fileActionAvailability } from "../app/fileActions";
import { deleteStep, prepareDelete } from "../app/graphActions";
import { useGenerationReady } from "../app/generationReady";
import type { OrganizeController } from "../app/useOrganize";
import type { WorkspaceState } from "../app/useWorkspace";
import type { DocumentRecord } from "../domain/contracts";
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
  const generationReady = useGenerationReady();
  const heading = useRef<HTMLHeadingElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  const availability = fileActionAvailability(workspace, document);
  const started = useRef(false);
  const step = deleteStep(state, {
    available: availability.available,
    started: started.current,
  });

  /** `fresh` reads the file again, so a stale preview isn't repeated. */
  function prepare(fresh: boolean) {
    started.current = true;
    action.previewFrom((folderId) =>
      prepareDelete(document, fresh, (listed) =>
        readNativeDocument(folderId, listed),
      ),
    );
  }

  useEffect(() => {
    if (availability.available) prepare(false);
    // Prepared once, when the dialog opens.
  }, []);

  // Cancel is the default in the preview; the result's heading is announced.
  useEffect(() => {
    if (step === "preview") cancel.current?.focus();
    else if (step === "result") heading.current?.focus();
    else if (step === "closed") close();
  }, [step]);

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
      {step === "unavailable" ? (
        <p>
          {availability.available ? null : availability.reason} Nothing was
          deleted.
        </p>
      ) : step === "result" ? (
        <>
          <ResultStep organize={action} heading={heading} onDone={close} />
          <p className="muted">
            Undo is here, and in Activity for as long as Folio keeps the file's
            contents.
          </p>
        </>
      ) : step === "preview" ? (
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
            onCancel={close}
            previewAgain={() => prepare(true)}
            details={
              state.plan ? (
                <ImpactList
                  impacts={state.plan.impacts}
                  generationReady={generationReady}
                  deletion
                />
              ) : null
            }
          />
        </>
      ) : step === "error" && state.error ? (
        <RecoveryNotice
          error={state.error}
          actions={{
            retry: () => prepare(true),
            previewAgain: () => prepare(true),
          }}
          onDismiss={close}
        />
      ) : (
        <Progress label="Preparing the exact preview" />
      )}
    </Modal>
  );
}

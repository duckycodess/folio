import { useEffect, useRef, useState } from "react";
import { simulatedFailure } from "../adapters/simulate";
import type { Drafts } from "../app/drafts";
import {
  fileName,
  folderChoices,
  folderPath,
  nameProblem,
} from "../app/fileActions";
import type { OrganizeController } from "../app/useOrganize";
import type { WorkspaceState } from "../app/useWorkspace";
import type { DocumentRecord } from "../domain/contracts";
import type { FolioError } from "../domain/errors";
import { Button } from "../ui/Button";
import { Modal } from "../ui/Modal";
import { Progress } from "../ui/Progress";
import { RecoveryNotice } from "../ui/RecoveryNotice";
import { PreviewStep, ResultStep } from "./OrganizeFlowPanel";

export type FileActionKind = "rename" | "move";

/**
 * Rename or move one file from Home: a small form, then the same exact native
 * preview, Approve, and result with Undo as Organize. Nothing is written until
 * the user approves the plan on screen.
 */
export function FileActionDialog({
  kind,
  document,
  workspace,
  drafts,
  action,
  onClose,
}: {
  kind: FileActionKind;
  document: DocumentRecord;
  workspace: WorkspaceState;
  drafts: Drafts;
  action: OrganizeController;
  onClose: () => void;
}) {
  const { state } = action;
  const heading = useRef<HTMLHeadingElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const select = useRef<HTMLSelectElement>(null);
  const current = fileName(document.relativePath);
  // The field owns its text (it may be empty while typing); the draft keeps
  // it if the dialog closes before the rename is saved.
  const [name, setName] = useState(
    () => drafts.renameName(document.id) || current,
  );
  const folders = folderChoices(workspace.documents);
  const [folder, setFolder] = useState(() => {
    const here = folderPath(document.relativePath);
    return folders.find((choice) => choice !== here) ?? here;
  });
  const problem =
    kind === "rename"
      ? nameProblem(name, current)
      : folder === folderPath(document.relativePath)
        ? "The file is already in that folder."
        : null;
  const inPlan = state.stage === "preview" || state.stage === "applying";
  // Sample files can't be changed, but practice mode (`?simulate=<code>`)
  // still shows how a refused change looks.
  const practice =
    workspace.source !== "folder" ? simulatedFailure("changes") : undefined;
  const [practiceError, setPracticeError] = useState<FolioError | null>(null);
  const error = practiceError ?? state.error;

  // Focus the field, with a name selected up to its extension like a file
  // manager does.
  function focusField() {
    if (kind === "move") return select.current?.focus();
    const field = input.current;
    if (!field) return;
    field.focus();
    const dot = field.value.lastIndexOf(".");
    field.setSelectionRange(0, dot > 0 ? dot : field.value.length);
  }

  useEffect(focusField, []);

  // A new step is announced by moving focus to its heading. Back from the
  // preview returns to the field, since the preview's buttons are gone.
  const lastStage = useRef(state.stage);
  useEffect(() => {
    const previous = lastStage.current;
    lastStage.current = state.stage;
    if (state.stage === "preview" || state.stage === "result")
      heading.current?.focus();
    else if (previous === "preview") focusField();
  }, [state.stage]);

  // A name typed for a file is done with once that rename is saved.
  useEffect(() => {
    const saved = state.report?.batch.outcomes.some(
      (outcome) => outcome.status === "succeeded",
    );
    if (saved && kind === "rename") drafts.clearRenameName(document.id);
    // Only a new report clears the draft.
  }, [state.report]);

  function preview() {
    if (problem) return;
    if (practice) return setPracticeError(practice);
    action.previewRelocate(document, kind === "rename" ? { name } : { folder });
  }

  const title = `${kind === "rename" ? "Rename" : "Move"} ${document.name}`;

  function close() {
    action.done();
    onClose();
  }

  return (
    <Modal
      open
      title={title}
      // The dialog stays open until an apply finishes, so its result (and
      // Undo) is never left in the flow for the next file's dialog.
      dismissible={state.stage !== "applying"}
      onClose={close}
    >
      {workspace.source !== "folder" && !practice ? (
        <p>
          {kind === "rename" ? "Renaming" : "Moving"} works on a folder you add
          {workspace.nativeAvailable ? "" : " in the desktop app"}. Sample files
          can't be changed, so nothing was changed.
        </p>
      ) : state.stage === "result" ? (
        <ResultStep organize={action} heading={heading} onDone={close} />
      ) : inPlan ? (
        <PreviewStep organize={action} heading={heading} cancelLabel="Back" />
      ) : (
        <form
          className="rename-form"
          onSubmit={(event) => {
            event.preventDefault();
            preview();
          }}
        >
          {kind === "rename" ? (
            <>
              <label htmlFor="file-action-name" className="field-label">
                New name
              </label>
              <input
                ref={input}
                id="file-action-name"
                className="text-input"
                value={name}
                aria-describedby="file-action-help"
                aria-invalid={problem !== null || undefined}
                onChange={(event) => {
                  setName(event.target.value);
                  drafts.setRenameName(document.id, event.target.value);
                }}
              />
            </>
          ) : (
            <>
              <label htmlFor="file-action-folder" className="field-label">
                Move to folder
              </label>
              <select
                ref={select}
                id="file-action-folder"
                className="text-input"
                value={folder}
                aria-describedby="file-action-help"
                onChange={(event) => setFolder(event.target.value)}
              >
                {folders.map((choice) => (
                  <option key={choice} value={choice}>
                    {choice || "Top of the folder"}
                  </option>
                ))}
              </select>
            </>
          )}
          <p id="file-action-help" className="field-help">
            {problem ?? "You'll see the exact change before anything happens."}
          </p>
          {error && (
            <RecoveryNotice
              error={error}
              actions={{
                retry: preview,
                previewAgain: preview,
                // In Move, another folder is the way to another name.
                chooseAnotherName: focusField,
              }}
              onDismiss={
                practiceError
                  ? () => setPracticeError(null)
                  : action.dismissError
              }
            />
          )}
          {state.stage === "preparing" ? (
            <Progress label="Preparing the exact preview" />
          ) : (
            <div className="form-actions">
              <Button type="submit" variant="primary" disabled={!!problem}>
                Preview {kind === "rename" ? "rename" : "move"}
              </Button>
            </div>
          )}
        </form>
      )}
    </Modal>
  );
}

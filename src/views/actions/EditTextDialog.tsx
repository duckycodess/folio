import { useEffect, useId, useRef, useState } from "react";
import { adoptRevision, asTyped, type EditBase } from "../../app/editDraft";
import { fileActionAvailability } from "../../app/fileActions";
import { usePlanAction } from "../../app/usePlanAction";
import type { WorkspaceState } from "../../app/useWorkspace";
import { readNativeDocument } from "../../adapters/workspace";
import type {
  DocumentRecord,
  FileOperation,
  NewPlanSource,
} from "../../domain/contracts";
import { toFolioError, type FolioError } from "../../domain/errors";
import { restoreLineEndings } from "../../domain/textDiff";
import { Button } from "../../ui/Button";
import { Notice } from "../../ui/Notice";
import { Progress } from "../../ui/Progress";
import { RecoveryNotice } from "../../ui/RecoveryNotice";
import { PlanReview } from "../PlanReview";
import { ActionDialog } from "./ActionDialog";

/**
 * Edit a TXT or Markdown file's text. The preview is the exact diff of the
 * native plan plus Ripple candidates as needs review; the file changes only
 * after the user approves that plan, and "saved" comes only from the native
 * report.
 */
export function EditTextDialog({
  open,
  document,
  workspace,
  onClose,
  source,
  onFilesChanged,
}: {
  open: boolean;
  document: DocumentRecord;
  workspace: WorkspaceState;
  onClose: () => void;
  /** Where in Folio the edit starts, for Activity. */
  source: NewPlanSource;
  onFilesChanged?: () => void;
}) {
  const action = usePlanAction(workspace, source, onFilesChanged);
  const [base, setBase] = useState<EditBase | null>(null);
  const [draft, setDraft] = useState("");
  const [reading, setReading] = useState(false);
  const [readError, setReadError] = useState<FolioError | null>(null);
  /** Set when the file changed under the draft; the draft is kept. */
  const [changedUnder, setChangedUnder] = useState(false);
  const request = useRef(0);
  const fieldId = useId();
  const available = fileActionAvailability(workspace, document).available;
  const folderId = workspace.workspace?.id;

  /** Reads the current revision. Resolves to it, or `null` after a failure. */
  async function read(): Promise<EditBase | null> {
    if (!folderId) return null;
    const current = ++request.current;
    setReading(true);
    setReadError(null);
    try {
      const fresh = await readNativeDocument(folderId, document);
      if (current !== request.current) return null;
      if (fresh.content === undefined || !fresh.contentHash)
        throw toFolioError(new Error("The file had no text."));
      return {
        documentId: document.id,
        content: fresh.content,
        contentHash: fresh.contentHash,
      };
    } catch (cause) {
      if (current === request.current) setReadError(toFolioError(cause));
      return null;
    } finally {
      if (current === request.current) setReading(false);
    }
  }

  /** Edit `fresh`, keeping a draft the user already started on this file. */
  function adopt(fresh: EditBase) {
    const next = adoptRevision({ base, draft }, fresh);
    if (!next) return;
    setBase(next.base);
    setDraft(next.draft);
    setChangedUnder(next.changedUnder);
  }

  // Opening reads the file afresh, so the edit pins its current revision.
  useEffect(() => {
    if (!open || !available) return;
    void read().then((fresh) => fresh && adopt(fresh));
  }, [open, document.id]);

  const after = base ? restoreLineEndings(base.content, draft) : "";
  const unchanged = !base || after === base.content;

  function preview() {
    if (!base || unchanged) return;
    const operation: FileOperation = {
      kind: "edit",
      documentId: base.documentId,
      relativePath: document.relativePath,
      expectedContentHash: base.contentHash,
      after,
    };
    // Without impacts, the native core computes Ripple from this edit's diff.
    action.prepare([operation]);
  }

  // A refused or expired preview may mean the file changed. Read it again: if
  // it didn't, preview the same edit again; if it did, keep the draft and say
  // so, because it was written against the earlier text.
  async function previewAgain() {
    const fresh = await read();
    if (!fresh) return;
    if (base && fresh.contentHash === base.contentHash) {
      action.previewAgain();
      return;
    }
    action.reset();
    adopt(fresh);
  }

  const stage = action.state.stage;

  return (
    <ActionDialog
      open={open}
      title={`Edit ${document.name}`}
      workspace={workspace}
      document={document}
      action={action}
      onClose={onClose}
    >
      {reading && <Progress label="Opening the file" />}
      {readError && (
        <RecoveryNotice
          error={readError}
          actions={{ retry: () => void previewAgain() }}
          onDismiss={() => setReadError(null)}
        />
      )}

      {base && stage === "idle" && (
        <form
          className="flow-step"
          onSubmit={(event) => {
            event.preventDefault();
            preview();
          }}
        >
          {changedUnder && (
            <Notice
              tone="warning"
              action={
                <Button
                  onClick={() => {
                    setDraft(asTyped(base.content));
                    setChangedUnder(false);
                  }}
                >
                  Start over from the current text
                </Button>
              }
            >
              <p className="notice-title">
                This file changed since you started editing
              </p>
              <p>
                Your text is kept, but it was written against the earlier
                version. Its preview will also show the other changes it would
                replace.
              </p>
            </Notice>
          )}
          <label htmlFor={fieldId} className="field-label">
            Text of {document.relativePath}
          </label>
          <textarea
            id={fieldId}
            className="text-area edit-text"
            value={draft}
            spellCheck={false}
            aria-describedby={`${fieldId}-help`}
            onChange={(event) => setDraft(event.target.value)}
          />
          <p id={`${fieldId}-help`} className="field-help">
            {unchanged
              ? "Change the text to preview it."
              : "You'll see the exact changes and any related passages before anything is saved."}
          </p>
          <div className="form-actions">
            <Button type="submit" variant="primary" disabled={unchanged}>
              Preview changes
            </Button>
            <Button variant="ghost" onClick={onClose}>
              Cancel
            </Button>
          </div>
        </form>
      )}

      {base && (
        <PlanReview
          action={action}
          inModal
          beforeText={{ [base.documentId]: base.content }}
          approveLabel="Approve and save"
          backLabel="Back to editing"
          onPreviewAgain={() => void previewAgain()}
          onBack={action.reset}
          onDone={onClose}
        />
      )}
    </ActionDialog>
  );
}

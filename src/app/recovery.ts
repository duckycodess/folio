import type { FolioErrorCode } from "../domain/contracts";
import type { FolioError } from "../domain/errors";

/** What the user can do next. Each screen wires only the actions it offers. */
export type RecoveryActionKind =
  | "retry"
  | "chooseFolder"
  | "openModelLab"
  | "previewAgain"
  | "chooseAnotherName"
  /** Stop what holds the local model, then run this request. */
  | "stopAndRetry";

/** Where an error comes from, and so where the browser preview can demo it. */
export type RecoveryFlow =
  "folder" | "read" | "changes" | "assistant" | "search";

/**
 * When a change-related error arrived, which decides what is true about the
 * files:
 * - `refused`: a check before any write turned the change down (prepare,
 *   approve, the apply gate, the Undo preflight). Nothing changed.
 * - `duringApply`: an operation failed partway through an approved batch.
 *   That file wasn't changed, but earlier operations in the batch were.
 * - `partialUndo`: an Undo stopped partway; what it already reversed stays
 *   reversed.
 */
export type RecoveryStage = "refused" | "duringApply" | "partialUndo";

export interface Recovery {
  tone: "danger" | "warning" | "info";
  title: string;
  message: string;
  action?: { kind: RecoveryActionKind; label: string };
  flow: RecoveryFlow;
  /** The code only ever follows a write, so the stage never changes its wording. */
  afterWrite?: true;
}

const NOTHING_CHANGED = "No file was changed.";
const EARLIER_KEPT =
  "This file wasn't changed. Earlier changes in this batch were kept and can be undone.";
const UNDO_PARTLY =
  "Folio undid part of this change, then stopped. Preview Undo again to finish.";

const TRY_AGAIN = { kind: "retry", label: "Try again" } as const;
const ADD_FOLDER_AGAIN = {
  kind: "chooseFolder",
  label: "Add the folder again",
} as const;
const OPEN_MODEL_LAB = {
  kind: "openModelLab",
  label: "Open Model Lab",
} as const;
const PREVIEW_AGAIN = { kind: "previewAgain", label: "Preview again" } as const;

/**
 * User wording for every error code on the boundary. A `Record` over the code
 * type, so a new code doesn't compile until it has wording here. Change-related
 * messages are written for the `refused` stage; `recoveryFor` rewords them for
 * the other stages.
 */
export const RECOVERY: Record<FolioErrorCode, Recovery> = {
  workspaceNotAuthorized: {
    tone: "danger",
    title: "Folio no longer has access to this folder",
    message: "Add the folder again to keep working with its files.",
    action: ADD_FOLDER_AGAIN,
    flow: "folder",
  },
  workspaceUnavailable: {
    tone: "danger",
    title: "This folder can't be reached",
    message:
      "It may have been moved, renamed or disconnected. Reconnect it, or add it again.",
    action: ADD_FOLDER_AGAIN,
    flow: "folder",
  },
  pathNotRelative: {
    tone: "danger",
    title: "Folio couldn't find that file in your folder",
    message: "Choose the file again from the list.",
    flow: "read",
  },
  pathEscapesWorkspace: {
    tone: "danger",
    title: "That file is outside the folders you added",
    message: "Folio only opens files inside folders you've chosen.",
    flow: "read",
  },
  pathUnsupportedEncoding: {
    tone: "warning",
    title: "This file name can't be read",
    message:
      "Its name uses characters Folio can't handle. Renaming it in your file manager will fix this.",
    flow: "read",
  },
  documentUnavailable: {
    tone: "danger",
    title: "This file couldn't be opened",
    message: "It may have been moved, deleted or opened by another app.",
    action: TRY_AGAIN,
    flow: "read",
  },
  documentTooLarge: {
    tone: "warning",
    title: "This file is too large to open here",
    message: "You can still find it in search by its name.",
    flow: "read",
  },
  documentNotText: {
    tone: "warning",
    title: "This file doesn't contain readable text",
    message:
      "It may be a scanned or image-only PDF. Folio reads text, Markdown and text-based PDF files.",
    flow: "read",
  },
  unsupportedMediaType: {
    tone: "warning",
    title: "Folio can't read this type of file",
    message: "Folio reads text, Markdown and text-based PDF files.",
    flow: "read",
  },
  planUnknown: {
    tone: "warning",
    title: "This preview is no longer available",
    message: `Preview the change again to continue. ${NOTHING_CHANGED}`,
    action: PREVIEW_AGAIN,
    flow: "changes",
  },
  planEmpty: {
    tone: "info",
    title: "There's nothing to change",
    message: "The preview didn't include any changes.",
    flow: "changes",
  },
  planExpired: {
    tone: "warning",
    title: "This preview has expired",
    message: `Previews are only valid for a short time. Preview the change again. ${NOTHING_CHANGED}`,
    action: PREVIEW_AGAIN,
    flow: "changes",
  },
  planStateInvalid: {
    tone: "warning",
    title: "This preview was already used or cancelled",
    message: `Preview the change again to continue. ${NOTHING_CHANGED}`,
    action: PREVIEW_AGAIN,
    flow: "changes",
  },
  planDigestMismatch: {
    tone: "danger",
    title: "The approved change doesn't match the preview",
    message: `Folio stopped to be safe. Preview the change again. ${NOTHING_CHANGED}`,
    action: PREVIEW_AGAIN,
    flow: "changes",
  },
  approvalRequired: {
    tone: "info",
    title: "This change needs your approval",
    message: `Review the preview and approve it to continue. ${NOTHING_CHANGED}`,
    action: PREVIEW_AGAIN,
    flow: "changes",
  },
  approvalStale: {
    tone: "warning",
    title: "Your approval is out of date",
    message: `Something changed after you approved. Review the preview again. ${NOTHING_CHANGED}`,
    action: PREVIEW_AGAIN,
    flow: "changes",
  },
  duplicateOperationTarget: {
    tone: "warning",
    title: "Two changes affect the same file",
    message: `Folio can make only one change to a file at a time. Edit the changes and preview again. ${NOTHING_CHANGED}`,
    action: PREVIEW_AGAIN,
    flow: "changes",
  },
  targetMissing: {
    tone: "warning",
    title: "A file in this change is missing",
    message: `It may have been moved or deleted since the preview. ${NOTHING_CHANGED}`,
    action: PREVIEW_AGAIN,
    flow: "changes",
  },
  targetChanged: {
    tone: "warning",
    title: "A file changed since the preview",
    message: `Review the new version before approving. ${NOTHING_CHANGED}`,
    action: PREVIEW_AGAIN,
    flow: "changes",
  },
  destinationExists: {
    tone: "warning",
    title: "A file with that name already exists",
    message: `Folio never replaces an existing file. Choose another name. ${NOTHING_CHANGED}`,
    action: { kind: "chooseAnotherName", label: "Choose another name" },
    flow: "changes",
  },
  operationUnsupported: {
    tone: "warning",
    title: "Folio can't make this kind of change",
    message: `Only text and Markdown files can be edited. PDFs can be read but not changed. ${NOTHING_CHANGED}`,
    flow: "changes",
  },
  // The native writer reports this only after the file was changed.
  historyRequired: {
    tone: "danger",
    title: "This file was changed, but Undo isn't available for it",
    message:
      "Folio saved the change but couldn't record how to reverse it. Keep a copy if you may need the earlier version.",
    flow: "changes",
    afterWrite: true,
  },
  historyUnknown: {
    tone: "warning",
    title: "Folio couldn't find this change in its history",
    message: `Undo isn't available for it. ${NOTHING_CHANGED}`,
    flow: "changes",
  },
  undoConflict: {
    tone: "warning",
    title: "These files changed after Folio's change",
    message: `Undo would overwrite newer edits, so Folio stopped. ${NOTHING_CHANGED}`,
    flow: "changes",
  },
  writerNotImplemented: {
    tone: "info",
    title: "Saving changes isn't available yet",
    message: `This version can preview changes but not save them. ${NOTHING_CHANGED}`,
    flow: "changes",
  },
  modelNotInstalled: {
    tone: "warning",
    title: "This needs a local AI model",
    message: "No model is set up yet. Your request is kept.",
    action: OPEN_MODEL_LAB,
    flow: "assistant",
  },
  modelLoadFailed: {
    tone: "danger",
    title: "The local AI model couldn't start",
    message:
      "Your computer may be short on memory. Close other apps and try again, or check the model in Model Lab. Your request is kept.",
    action: OPEN_MODEL_LAB,
    flow: "assistant",
  },
  providerBusy: {
    tone: "info",
    title: "Folio is still working on another request",
    message: "Try again when it finishes. Your request is kept.",
    action: TRY_AGAIN,
    flow: "assistant",
  },
  cancelled: {
    tone: "info",
    title: "Stopped",
    message: "The request was cancelled. Your request is kept.",
    action: TRY_AGAIN,
    flow: "assistant",
  },
  contextOverflow: {
    tone: "warning",
    title: "That's too much text to work with at once",
    message:
      "Try a shorter request, or fewer or smaller files. Your request is kept.",
    flow: "assistant",
  },
  embeddingSpaceMismatch: {
    tone: "warning",
    title: "Search needs to be refreshed",
    message:
      "Your files were prepared for search with a different model. Keyword search still works.",
    action: OPEN_MODEL_LAB,
    flow: "search",
  },
  evidenceInvalid: {
    tone: "warning",
    title: "The passage this relied on has changed",
    message:
      "The file was edited since Folio read it. Search again to use the current text.",
    action: TRY_AGAIN,
    flow: "search",
  },
  internal: {
    tone: "danger",
    title: "Something went wrong",
    message:
      "Folio couldn't finish this. Your work is kept, and you can try again.",
    action: TRY_AGAIN,
    flow: "read",
  },
};

/** Wording for an Undo that stopped partway, where it differs from the default. */
const PARTIAL_UNDO: Partial<Record<FolioErrorCode, string>> = {
  undoConflict:
    "Folio undid part of this change, then stopped because a file changed afterwards. Preview Undo again to finish.",
};

/** What the native core says holds the local model (`providerBusy`'s `holder`). */
const BUSY_HOLDER: Record<string, string> = {
  summary: "Folio is writing a summary",
  answer: "Folio is answering another question",
  interpretation: "Folio is reading another request",
  relationshipSummary: "Folio is summarizing connections in Graph",
  impactExplanation: "Folio is explaining a related file in a preview",
  organizeSuggestions: "Folio is naming Organize's suggestions",
  modelLab: "Model Lab is measuring models",
};

/**
 * The wording and next step for any failure, known code or not. `stage`
 * matters only for change-related codes; see `RecoveryStage`.
 */
export function recoveryFor(
  error: Pick<FolioError, "code"> & Partial<Pick<FolioError, "details">>,
  stage: RecoveryStage = "refused",
): Recovery {
  const base = RECOVERY[error.code] ?? RECOVERY.internal;
  const holder = error.details?.holder;
  if (error.code === "providerBusy" && holder && BUSY_HOLDER[holder])
    return {
      ...base,
      title: BUSY_HOLDER[holder],
      message:
        "Folio runs one AI task at a time. Stop it to run your request now, or try again when it finishes. Your request is kept.",
      action: { kind: "stopAndRetry", label: "Stop it and try again" },
    };
  if (base.flow !== "changes" || base.afterWrite || stage === "refused")
    return base;
  const reason = base.message.replace(` ${NOTHING_CHANGED}`, "");
  // An Undo error never comes from applying a plan; partway, it is a partial Undo.
  if (
    stage === "partialUndo" ||
    (stage === "duringApply" && error.code === "undoConflict")
  )
    return {
      ...base,
      message: PARTIAL_UNDO[error.code] ?? `${reason} ${UNDO_PARTLY}`,
      action: { kind: "previewAgain", label: "Preview Undo again" },
    };
  // During apply the plan is spent, so retrying it can't help; a new preview can.
  return {
    ...base,
    message: `${reason} ${EARLIER_KEPT}`,
    action: base.action?.kind === "retry" ? undefined : base.action,
  };
}

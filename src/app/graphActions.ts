import type { DocumentRecord, FileOperation } from "../domain/contracts";
import { folioError } from "../domain/errors";
import { fileActionAvailability } from "./fileActions";
import type { OrganizeState } from "./organizeFlow";
import type { WorkspaceState } from "./useWorkspace";

/** What a file on the Graph map can do besides opening in the reader. */
export type GraphActionKind = "rename" | "move" | "edit" | "delete";

export interface GraphAction {
  kind: GraphActionKind;
  label: string;
  /** Why it can't be used here; absent when it can. */
  disabledReason?: string;
}

const LABELS: Record<GraphActionKind, string> = {
  rename: "Rename…",
  move: "Move to folder…",
  edit: "Edit text…",
  delete: "Delete…",
};

/**
 * Every action a map node offers, in menu order. They are always listed, so
 * people learn they exist, but sample files, the browser preview and PDFs
 * can only be opened: there each action carries the reason instead.
 */
export function graphActions(
  workspace: Pick<WorkspaceState, "source" | "nativeAvailable">,
  document: Pick<DocumentRecord, "mediaType">,
): GraphAction[] {
  const availability = fileActionAvailability(workspace, document);
  return (Object.keys(LABELS) as GraphActionKind[]).map((kind) => ({
    kind,
    label: LABELS[kind],
    disabledReason: availability.available ? undefined : availability.reason,
  }));
}

/**
 * The plan operation that deletes one file. It pins the revision Folio read,
 * so the native core refuses it if the file changed since.
 */
export function deleteOperation(
  document: Pick<DocumentRecord, "id" | "relativePath"> & {
    contentHash: string;
  },
): Extract<FileOperation, { kind: "delete" }> {
  return {
    kind: "delete",
    documentId: document.id,
    relativePath: document.relativePath,
    expectedContentHash: document.contentHash,
  };
}

/**
 * The delete operation for a file, pinned to its current revision. The
 * listing's hash is used when it has one; `fresh` (Preview again, Retry)
 * always reads the file again, so a refused stale preview can't repeat.
 */
export async function prepareDelete(
  document: DocumentRecord,
  fresh: boolean,
  read: (document: DocumentRecord) => Promise<DocumentRecord>,
): Promise<FileOperation[]> {
  const current =
    !fresh && document.contentHash ? document : await read(document);
  if (!current.contentHash)
    throw folioError("internal", "Folio couldn't read this file's hash.");
  return [deleteOperation({ ...current, contentHash: current.contentHash })];
}

/** What the Delete dialog shows, from the plan flow it drives. */
export type DeleteStep =
  | "unavailable"
  | "preparing"
  | "preview"
  | "result"
  | "error"
  /** Left without a result: Cancel, or the folder changed underneath. */
  | "closed";

export function deleteStep(
  state: Pick<OrganizeState, "stage" | "error">,
  {
    available,
    started,
  }: {
    available: boolean;
    /** The dialog has asked for its preview (until then the flow is idle). */
    started: boolean;
  },
): DeleteStep {
  if (!available) return "unavailable";
  switch (state.stage) {
    case "result":
      return "result";
    case "preview":
    case "applying":
      return "preview";
    case "idle":
    case "suggestions":
      // A preview that failed to build rests here with its error.
      return state.error ? "error" : started ? "closed" : "preparing";
    default:
      return "preparing";
  }
}

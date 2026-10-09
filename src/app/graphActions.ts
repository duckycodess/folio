import type { DocumentRecord, FileOperation } from "../domain/contracts";
import { fileActionAvailability } from "./fileActions";
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

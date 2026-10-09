import {
  EDITABLE_MEDIA_TYPES,
  type DocumentRecord,
  type FileOperation,
} from "../domain/contracts";
import { mediaTypeForPath } from "../domain/identity";
import type { WorkspaceState } from "./useWorkspace";

export type FileActionAvailability =
  { available: true } | { available: false; reason: string };

/**
 * Whether a file's text, name or folder can be changed. These are early hints
 * so the user isn't sent to a refusal; the native core still checks every
 * plan itself. Changes need the desktop app and a folder the user added:
 * sample files and the browser preview never pretend to save. Folio changes
 * only text and Markdown files, so a PDF can only be opened.
 */
export function fileActionAvailability(
  workspace: Pick<WorkspaceState, "source" | "nativeAvailable">,
  document: Pick<DocumentRecord, "mediaType">,
): FileActionAvailability {
  if (!workspace.nativeAvailable)
    return {
      available: false,
      reason:
        "Changing files works in the desktop app. This preview only shows sample files.",
    };
  if (workspace.source !== "folder")
    return {
      available: false,
      reason:
        "Sample files can't be changed. Add a folder to change its files.",
    };
  if (!EDITABLE_MEDIA_TYPES.includes(document.mediaType as never))
    return {
      available: false,
      reason:
        "PDFs can only be opened. Folio changes text and Markdown files only.",
    };
  return { available: true };
}

/** The folder part of a relative path; "" for the top of the open folder. */
export function folderPath(relativePath: string): string {
  return relativePath.split("/").slice(0, -1).join("/");
}

export function fileName(relativePath: string): string {
  return relativePath.split("/").at(-1) ?? relativePath;
}

/**
 * Folders a file can move to: every folder inside the open folder that holds
 * a listed file, plus the top, sorted. Never anything outside the folder.
 */
export function folderChoices(documents: DocumentRecord[]): string[] {
  const folders = new Set<string>([""]);
  for (const document of documents) {
    const parts = folderPath(document.relativePath).split("/").filter(Boolean);
    for (let depth = 1; depth <= parts.length; depth++)
      folders.add(parts.slice(0, depth).join("/"));
  }
  return [...folders].sort((a, b) => a.localeCompare(b));
}

/** Why a typed file name can't be used, or `null` when it can. */
export function nameProblem(name: string, current: string): string | null {
  const trimmed = name.trim();
  if (!trimmed) return "Type a new name.";
  if (/[\\/]/.test(trimmed)) return "A file name can't contain / or \\.";
  if (trimmed === "." || trimmed === "..") return "Choose a different name.";
  if (trimmed === current) return "That's already the file's name.";
  // Windows and macOS folders usually ignore case, so the native plan refuses
  // a rename that only changes capital letters.
  if (trimmed.toLowerCase() === current.toLowerCase())
    return "A new name must differ by more than capital letters.";
  // The native plan refuses a destination it couldn't edit afterwards.
  const mediaType = mediaTypeForPath(trimmed);
  if (!mediaType || !EDITABLE_MEDIA_TYPES.includes(mediaType as never))
    return "Keep a .md, .markdown or .txt ending. Folio changes text and Markdown files only.";
  return null;
}

/**
 * A rename keeps the folder and changes the name; a move keeps the name and
 * changes the folder. Either way the plan pins the revision Folio read.
 */
export function relocateOperation(
  document: DocumentRecord & { contentHash: string },
  change: { name: string } | { folder: string },
): Extract<FileOperation, { kind: "rename" | "move" }> {
  const rename = "name" in change;
  const folder = rename ? folderPath(document.relativePath) : change.folder;
  const name = rename ? change.name.trim() : fileName(document.relativePath);
  return {
    kind: rename ? "rename" : "move",
    documentId: document.id,
    relativePath: document.relativePath,
    expectedContentHash: document.contentHash,
    destinationRelativePath: folder ? `${folder}/${name}` : name,
    expectedDestination: "absent",
  };
}

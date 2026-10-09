import {
  EDITABLE_MEDIA_TYPES,
  type DocumentRecord,
  type RelativePath,
} from "../domain/contracts";
import { mediaTypeForPath } from "../domain/identity";
import type { WorkspaceState } from "./useWorkspace";

/**
 * What the Edit text, Rename and Move actions may offer for a file. These are
 * early hints so the user isn't sent to a refusal; the native core still
 * checks every plan itself.
 */
export type FileActionKind = "edit" | "rename" | "move";

export type FileActionAvailability =
  { available: true } | { available: false; reason: string };

/**
 * Changes need the desktop app and a folder the user added: sample files and
 * the browser preview never pretend to save. Folio changes only text and
 * Markdown files, so a PDF can only be opened.
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

/** Why a typed name can't be previewed, or `null` when it can. */
export function renameProblem(
  relativePath: RelativePath,
  name: string,
): string | null {
  const trimmed = name.trim();
  if (!trimmed) return "Type a new name.";
  if (/[\\/]/.test(trimmed)) return "A file name can't contain / or \\.";
  const current = relativePath.split("/").at(-1) ?? relativePath;
  // Windows and macOS usually treat names that differ only in case as the same.
  if (trimmed.toLowerCase() === current.toLowerCase())
    return "That's the file's current name.";
  const mediaType = mediaTypeForPath(trimmed);
  if (!mediaType || !EDITABLE_MEDIA_TYPES.includes(mediaType as never))
    return "Keep a .md, .markdown or .txt ending. Folio changes text and Markdown files only.";
  return null;
}

export interface MoveFolder {
  /** Workspace-relative folder; `""` is the top of the folder the user added. */
  folder: RelativePath;
  /** Why the file can't go there, when it can't. */
  blocked?: string;
}

/**
 * Folders a file can move to: only folders that already hold files Folio
 * lists, because a move never creates a folder. The file's own folder isn't
 * offered, and a folder that already has a file of the same name is marked.
 */
export function moveFolders(
  documents: Pick<DocumentRecord, "relativePath">[],
  document: Pick<DocumentRecord, "relativePath">,
): MoveFolder[] {
  const folderOf = (path: string) => path.split("/").slice(0, -1).join("/");
  const nameOf = (path: string) =>
    (path.split("/").at(-1) ?? path).toLowerCase();
  const folders = new Set<string>([""]);
  for (const item of documents) {
    const parts = item.relativePath.split("/").slice(0, -1);
    for (let depth = 1; depth <= parts.length; depth++)
      folders.add(parts.slice(0, depth).join("/"));
  }
  const own = folderOf(document.relativePath);
  const name = nameOf(document.relativePath);
  const taken = new Set(
    documents
      .filter((item) => nameOf(item.relativePath) === name)
      .map((item) => folderOf(item.relativePath).toLowerCase()),
  );
  return [...folders]
    .filter((folder) => folder !== own)
    .sort((a, b) => a.localeCompare(b))
    .map((folder) =>
      taken.has(folder.toLowerCase())
        ? { folder, blocked: "Already has a file with this name" }
        : { folder },
    );
}

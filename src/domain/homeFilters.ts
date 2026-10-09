import type { DocumentRecord, MediaType } from "./contracts";

export type TypeFilter = "any" | MediaType;
export type ModifiedFilter = "any" | "today" | "week" | "month";

/** Home's explicit filters. They combine with the search query. */
export interface HomeFilters {
  /** A folder path inside the open folder ("" is its top level), or `null` for all. */
  folder: string | null;
  type: TypeFilter;
  modified: ModifiedFilter;
}

export const NO_FILTERS: HomeFilters = {
  folder: null,
  type: "any",
  modified: "any",
};

const DAY_MS = 24 * 60 * 60 * 1000;

export function hasFilters(filters: HomeFilters): boolean {
  return (
    filters.folder !== null ||
    filters.type !== "any" ||
    filters.modified !== "any"
  );
}

/** Folder part of a relative path, "" for the top level. */
export function folderPath(relativePath: string): string {
  const slash = relativePath.lastIndexOf("/");
  return slash === -1 ? "" : relativePath.slice(0, slash);
}

/** Every folder that holds a listed file, and their parents, sorted. */
export function foldersOf(documents: DocumentRecord[]): string[] {
  const folders = new Set<string>();
  for (const document of documents) {
    const parts = folderPath(document.relativePath).split("/").filter(Boolean);
    for (let depth = 1; depth <= parts.length; depth++)
      folders.add(parts.slice(0, depth).join("/"));
  }
  return [...folders].sort((a, b) => a.localeCompare(b));
}

function startOfToday(now: number): number {
  const date = new Date(now);
  date.setHours(0, 0, 0, 0);
  return date.getTime();
}

/**
 * Whether a file passes the filters. A folder includes its subfolders. A file
 * with no recorded modification time only passes "any time", so a date
 * filter never guesses.
 */
export function passesFilters(
  document: DocumentRecord,
  filters: HomeFilters,
  now: number,
): boolean {
  if (filters.folder !== null) {
    const folder = folderPath(document.relativePath);
    if (
      filters.folder === ""
        ? folder !== ""
        : folder !== filters.folder && !folder.startsWith(`${filters.folder}/`)
    )
      return false;
  }
  if (filters.type !== "any" && document.mediaType !== filters.type)
    return false;
  if (filters.modified !== "any") {
    if (document.modifiedAtMs === undefined) return false;
    const since =
      filters.modified === "today"
        ? startOfToday(now)
        : now - (filters.modified === "week" ? 7 : 30) * DAY_MS;
    if (document.modifiedAtMs < since) return false;
  }
  return true;
}

/** Recently opened files, newest first: `id` moves to the front, at most `limit`. */
export function rememberRecent(
  recent: string[],
  id: string,
  limit = 8,
): string[] {
  return [id, ...recent.filter((item) => item !== id)].slice(0, limit);
}

/** Pins a folder, or unpins it when it is already pinned. */
export function togglePin(pins: string[], folder: string): string[] {
  return pins.includes(folder)
    ? pins.filter((item) => item !== folder)
    : [...pins, folder].sort((a, b) => a.localeCompare(b));
}

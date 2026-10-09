import type { DocumentRecord, SearchResult } from "../domain/contracts";
import type { ViewId } from "./navigation";

/**
 * Views whose main content is a document list, so the reader sits beside it
 * (or, in narrow windows, takes its place). Organize is not one: its rename
 * form must stay visible next to the chosen file.
 */
const DOCUMENT_VIEWS = new Set<ViewId>(["home", "files", "graph"]);

/**
 * The document the reader should show, if any. A file is never shown beside
 * a list that doesn't include it: when a search excludes the open file, the
 * reader closes until the search changes. Graph lists links, not search
 * results, so any file opened from it is shown.
 */
export function readerDocument(
  view: ViewId,
  selected: DocumentRecord | undefined,
  results: SearchResult[],
): DocumentRecord | undefined {
  if (!selected || !DOCUMENT_VIEWS.has(view)) return undefined;
  if (view === "graph") return selected;
  return results.some((result) => result.document.id === selected.id)
    ? selected
    : undefined;
}

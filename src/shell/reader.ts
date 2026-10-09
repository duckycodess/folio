import type { DocumentRecord, SearchResult } from "../domain/contracts";
import type { ViewId } from "./navigation";

/**
 * Views whose main content is a document list, so the reader sits beside it
 * (or, in narrow windows, takes its place). Organize is not one: it works on
 * its own plan rather than on the chosen file.
 */
const DOCUMENT_VIEWS = new Set<ViewId>(["home", "graph", "assistant"]);

/**
 * The chosen file, if the current search results include it. A search that
 * leaves the file out hides it everywhere (the reader and its
 * file actions) until the search changes, so nothing acts on a file that isn't on screen.
 */
export function listedSelection(
  selected: DocumentRecord | undefined,
  results: SearchResult[],
): DocumentRecord | undefined {
  return selected &&
    results.some((result) => result.document.id === selected.id)
    ? selected
    : undefined;
}

/**
 * The document the reader should show, if any. A file is never shown beside
 * a list that doesn't include it: when a search excludes the open file, the
 * reader closes until the search changes. Graph lists links, not search
 * results, and Ask & Act shows its own, so any file opened from them is shown.
 */
export function readerDocument(
  view: ViewId,
  selected: DocumentRecord | undefined,
  results: SearchResult[],
): DocumentRecord | undefined {
  if (!selected || !DOCUMENT_VIEWS.has(view)) return undefined;
  if (view === "graph" || view === "assistant") return selected;
  return listedSelection(selected, results);
}

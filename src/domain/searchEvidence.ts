import type { DocumentRecord, SearchResult } from "./contracts";

/**
 * Search results for an open folder: the persistent index's text matches
 * first, then files whose names match but whose text the index doesn't hold.
 * Index results are rebound to the folder's current records, and a result for
 * a file that is no longer listed is dropped, so nothing unlisted is shown.
 */
export function mergeFolderResults(
  documents: DocumentRecord[],
  indexResults: SearchResult[],
  nameResults: SearchResult[],
): SearchResult[] {
  const byId = new Map(documents.map((document) => [document.id, document]));
  const merged: SearchResult[] = [];
  const seen = new Set<string>();
  for (const result of indexResults) {
    const document = byId.get(result.document.id);
    if (!document || seen.has(document.id)) continue;
    seen.add(document.id);
    merged.push({ ...result, document });
  }
  for (const result of nameResults) {
    if (seen.has(result.document.id) || !byId.has(result.document.id)) continue;
    seen.add(result.document.id);
    merged.push(result);
  }
  return merged;
}

/** How a result matched, in words a reader can use. Never "semantic" for keywords. */
export function matchLabel(result: SearchResult): string {
  switch (result.method) {
    case "semantic":
      return "Similar meaning";
    case "hybrid":
      return "Words and meaning";
    default:
      return result.passages.length ? "Words in the text" : "Words in the name";
  }
}

export interface Segment {
  text: string;
  hit: boolean;
}

/** Case- and accent-insensitive, like the index (e.g. "nino" finds "Niño"). */
function fold(value: string): string {
  return value.normalize("NFD").replace(/\p{M}/gu, "").toLocaleLowerCase();
}

function queryTerms(query: string): string[] {
  return [
    ...new Set(fold(query.normalize("NFKC")).match(/[\p{L}\p{N}]+/gu) ?? []),
  ];
}

/**
 * Splits an excerpt into plain and matching segments for the query's words.
 * A word matches where a word starts, so "plan" marks "plan" and "planning"
 * but not "explanation".
 */
export function highlightSegments(text: string, query: string): Segment[] {
  const terms = queryTerms(query);
  if (!terms.length || !text) return text ? [{ text, hit: false }] : [];
  // Fold character by character, remembering where each folded character came from.
  let folded = "";
  const origin: number[] = [];
  for (let index = 0; index < text.length;) {
    const char = String.fromCodePoint(text.codePointAt(index)!);
    const piece = fold(char);
    for (let k = 0; k < piece.length; k++) origin.push(index);
    folded += piece;
    index += char.length;
  }
  origin.push(text.length);

  const marks = new Array<boolean>(text.length).fill(false);
  const isWord = (char: string | undefined) =>
    char !== undefined && /[\p{L}\p{N}]/u.test(char);
  for (const term of terms) {
    let at = folded.indexOf(term);
    while (at !== -1) {
      if (!isWord(folded[at - 1])) {
        // Extend to the end of the word, so the whole matching word is marked.
        let end = at + term.length;
        while (end < folded.length && isWord(folded[end])) end++;
        for (let i = origin[at]; i < origin[end]; i++) marks[i] = true;
      }
      at = folded.indexOf(term, at + 1);
    }
  }

  const segments: Segment[] = [];
  for (let i = 0; i < text.length; i++) {
    const last = segments.at(-1);
    if (last && last.hit === marks[i]) last.text += text[i];
    else segments.push({ text: text[i], hit: marks[i] });
  }
  return segments;
}

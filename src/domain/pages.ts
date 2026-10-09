import { utf8Length, utf8OffsetToUtf16Index } from "./offsets";

/** One PDF page's text in the read content, as UTF-8 byte offsets (like passages). */
export interface PageRange {
  page: number;
  start: number;
  end: number;
}

/** What the reader shows, in page order. Offsets here are string indices. */
export type ReaderBlock =
  | { kind: "page"; page: number; start: number; end: number }
  | { kind: "unreadable"; page: number };

/**
 * Lays a paged document out for the reader: each page's text, and a block for
 * every page whose text couldn't be extracted, in page order. Returns `null`
 * for unpaged text (TXT, Markdown), or when the ranges don't fit the content,
 * so the reader shows the text as one block rather than mislabel a page.
 */
export function readerBlocks(
  content: string,
  pages: PageRange[] | undefined,
  unreadablePages: number[] = [],
): ReaderBlock[] | null {
  if (!pages?.length) return null;
  const length = utf8Length(content);
  let previousEnd = 0;
  const blocks: ReaderBlock[] = [];
  try {
    for (const range of pages) {
      if (
        !Number.isInteger(range.page) ||
        range.start < previousEnd ||
        range.end < range.start ||
        range.end > length
      )
        return null;
      previousEnd = range.end;
      blocks.push({
        kind: "page",
        page: range.page,
        start: utf8OffsetToUtf16Index(content, range.start),
        end: utf8OffsetToUtf16Index(content, range.end),
      });
    }
  } catch {
    // An offset inside a character: the ranges don't belong to this text.
    return null;
  }
  const listed = new Set(pages.map((range) => range.page));
  for (const page of unreadablePages)
    if (!listed.has(page)) blocks.push({ kind: "unreadable", page });
  return blocks.sort((a, b) => a.page - b.page);
}

/** The page of `blocks` that holds string index `index`, if any. */
export function pageAt(
  blocks: ReaderBlock[],
  index: number,
): number | undefined {
  return blocks.find(
    (block) =>
      block.kind === "page" && index >= block.start && index < block.end,
  )?.page;
}

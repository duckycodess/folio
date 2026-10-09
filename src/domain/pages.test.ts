import { describe, expect, it } from "vitest";
import { utf8Length } from "./offsets";
import { pageAt, readerBlocks, type PageRange } from "./pages";

// Two pages joined by a blank line, as the native reader returns them.
const first = "Gabay sa pahintulot (consent).";
const second = "Pirmahan ang form — salamat, Niño.";
const content = `${first}\n\n${second}`;
const pages: PageRange[] = [
  { page: 1, start: 0, end: utf8Length(first) },
  {
    page: 2,
    start: utf8Length(`${first}\n\n`),
    end: utf8Length(content),
  },
];

describe("reader pages", () => {
  it("shows unpaged text as one block", () => {
    expect(readerBlocks("Plain notes", undefined)).toBeNull();
    expect(readerBlocks("Plain notes", [])).toBeNull();
  });

  it("maps UTF-8 page ranges onto the text, with non-ASCII characters", () => {
    const blocks = readerBlocks(content, pages)!;
    expect(
      blocks.map((block) =>
        block.kind === "page" ? content.slice(block.start, block.end) : "",
      ),
    ).toEqual([first, second]);
  });

  it("lists pages that couldn't be read in page order", () => {
    const blocks = readerBlocks(
      content,
      [pages[0], { ...pages[1], page: 3 }],
      [2],
    )!;
    expect(blocks.map((block) => `${block.kind}:${block.page}`)).toEqual([
      "page:1",
      "unreadable:2",
      "page:3",
    ]);
  });

  it("refuses ranges that don't fit the text instead of mislabelling pages", () => {
    expect(
      readerBlocks(content, [
        { page: 1, start: 0, end: utf8Length(content) + 5 },
      ]),
    ).toBeNull();
    expect(readerBlocks(content, [pages[1], pages[0]])).toBeNull();
    // An offset inside "—" (three bytes in UTF-8).
    const dash = utf8Length(`${first}\n\nPirmahan ang form `) + 1;
    expect(
      readerBlocks(content, [{ page: 1, start: 0, end: dash }]),
    ).toBeNull();
  });

  it("finds the page that holds a passage", () => {
    const blocks = readerBlocks(content, pages)!;
    expect(pageAt(blocks, content.indexOf("Niño"))).toBe(2);
    expect(pageAt(blocks, 0)).toBe(1);
    // The blank line between pages belongs to neither.
    expect(pageAt(blocks, first.length)).toBeUndefined();
  });
});

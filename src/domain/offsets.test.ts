import { describe, expect, it } from "vitest";
import {
  assertPassageMatches,
  passageFromUtf16Range,
  sliceByUtf8Offsets,
  utf16IndexToUtf8Offset,
  utf8Length,
  utf8OffsetToUtf16Index,
} from "./offsets";
import { hashText } from "./hash";
import { isFolioError } from "./errors";
import { SOURCE_OFFSET_UNIT } from "./contracts";

const TAGLISH = "Hanapin yung project plan ni Niña at palitan ang deadline.";
const MIXED = "Deadline \u{1f4c5} na\u0303 October 20 \u2192 October 23";

describe("source offsets", () => {
  it("uses UTF-8 bytes, not JavaScript string indexes", () => {
    expect(utf8Length(TAGLISH)).toBe(59);
    expect(TAGLISH.length).toBe(58);
    expect(utf8Length(MIXED)).toBe(44);
    expect(MIXED.length).toBe(39);
  });

  it("round-trips between string indexes and boundary offsets", () => {
    for (const text of [TAGLISH, MIXED]) {
      for (let index = 0; index <= text.length; index += 1) {
        if (
          index > 0 &&
          index < text.length &&
          text.codePointAt(index - 1)! > 0xffff
        ) {
          continue; // Inside a surrogate pair.
        }
        const offset = utf16IndexToUtf8Offset(text, index);
        expect(utf8OffsetToUtf16Index(text, offset)).toBe(index);
      }
    }
  });

  it("refuses an offset inside a character rather than returning mojibake", () => {
    const emojiStart = utf8Length("Deadline ");
    let code = "no-error";
    try {
      sliceByUtf8Offsets(MIXED, emojiStart + 1, utf8Length(MIXED));
    } catch (cause) {
      code = isFolioError(cause) ? cause.code : String(cause);
    }
    expect(code).toBe("internal");
  });

  it("builds a passage whose text is exactly its byte range", async () => {
    const contentHash = await hashText(MIXED);
    const passage = passageFromUtf16Range({
      documentId: "w:notes/paalala.md",
      documentContentHash: contentHash,
      text: MIXED,
      startIndex: 0,
      endIndex: MIXED.indexOf("→"),
    });
    expect(passage.offsetUnit).toBe(SOURCE_OFFSET_UNIT);
    expect(sliceByUtf8Offsets(MIXED, passage.start, passage.end)).toBe(
      passage.text,
    );
    assertPassageMatches(passage, MIXED, contentHash);
  });

  it("detects evidence taken from an earlier revision", async () => {
    const original = "Deadline: October 20";
    const edited = "Deadline: October 23";
    const passage = passageFromUtf16Range({
      documentId: "w:projects/project-plan.md",
      documentContentHash: await hashText(original),
      text: original,
      startIndex: 0,
      endIndex: original.length,
    });
    let code = "no-error";
    try {
      assertPassageMatches(passage, edited, await hashText(edited));
    } catch (cause) {
      code = isFolioError(cause) ? cause.code : String(cause);
    }
    expect(code).toBe("evidenceInvalid");
  });
});

import { describe, expect, it } from "vitest";
import type { ContentHash } from "../domain/contracts";
import { adoptRevision, asTyped, type EditBase } from "./editDraft";

const revision = (content: string, hash: string): EditBase => ({
  documentId: "w:notes/plan.md",
  content,
  contentHash: `sha256:${hash.repeat(64)}` as ContentHash,
});
const first = revision("Deadline: May 20\r\n", "a");

describe("adopting a fresh read of the file being edited", () => {
  it("starts from the file's text, as a textarea shows it", () => {
    expect(adoptRevision({ base: null, draft: "" }, first)).toEqual({
      base: first,
      draft: "Deadline: May 20\n",
      changedUnder: false,
    });
    expect(asTyped("a\r\nb\rc\n")).toBe("a\nb\nc\n");
  });

  it("changes nothing when the same revision is read again", () => {
    expect(
      adoptRevision({ base: first, draft: "Deadline: May 25\n" }, first),
    ).toBeNull();
  });

  it("keeps a started draft and warns when the file changed under it", () => {
    const theirs = revision("Deadline: May 21\r\n", "b");
    expect(
      adoptRevision({ base: first, draft: "Deadline: May 25\n" }, theirs),
    ).toEqual({
      base: theirs,
      draft: "Deadline: May 25\n",
      changedUnder: true,
    });
  });

  it("takes the new text without a warning when no draft was started", () => {
    const theirs = revision("Deadline: May 21\r\n", "b");
    expect(
      adoptRevision({ base: first, draft: "Deadline: May 20\n" }, theirs),
    ).toEqual({
      base: theirs,
      draft: "Deadline: May 21\n",
      changedUnder: false,
    });
  });

  it("doesn't warn about the user's own saved edit", () => {
    const saved = revision("Deadline: May 25\r\n", "c");
    expect(
      adoptRevision({ base: first, draft: "Deadline: May 25\n" }, saved),
    ).toEqual({
      base: saved,
      draft: "Deadline: May 25\n",
      changedUnder: false,
    });
  });
});

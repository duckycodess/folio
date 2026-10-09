import { describe, expect, it } from "vitest";
import {
  diffLines,
  restoreLineEndings,
  splitLines,
  type DiffLine,
  type LineDiff,
} from "./textDiff";

function changed(diff: LineDiff): Pick<DiffLine, "kind" | "text">[] {
  if (diff.tooLarge) throw new Error("expected a diff");
  return diff.hunks
    .flatMap((hunk) => hunk.lines)
    .filter((line) => line.kind !== "same")
    .map(({ kind, text }) => ({ kind, text }));
}

const PLAN = [
  "# Project plan",
  "",
  "Owner: Niña",
  "Deadline: October 20",
  "Budget: ₱12,000",
  "",
  "Paalala: ipasa bago mag-alas singko.",
].join("\n");

describe("line diffs", () => {
  it("has no hunks for identical text", () => {
    expect(diffLines(PLAN, PLAN)).toEqual({ tooLarge: false, hunks: [] });
    expect(diffLines("", "")).toEqual({ tooLarge: false, hunks: [] });
  });

  it("shows a replaced line as removed, then added, with line numbers", () => {
    const diff = diffLines(PLAN, PLAN.replace("October 20", "October 23"), {
      context: 1,
    });
    if (diff.tooLarge) throw new Error("expected a diff");
    expect(diff.hunks).toHaveLength(1);
    expect(diff.hunks[0].lines).toEqual([
      {
        kind: "same",
        text: "Owner: Niña",
        ending: "\n",
        beforeLine: 3,
        afterLine: 3,
      },
      {
        kind: "removed",
        text: "Deadline: October 20",
        ending: "\n",
        beforeLine: 4,
      },
      {
        kind: "added",
        text: "Deadline: October 23",
        ending: "\n",
        afterLine: 4,
      },
      {
        kind: "same",
        text: "Budget: ₱12,000",
        ending: "\n",
        beforeLine: 5,
        afterLine: 5,
      },
    ]);
  });

  it("keeps Filipino and other multibyte text intact", () => {
    const before = "Pulong sa Miyerkules 🗓️\nPaksa: badyet ng proyekto\n";
    const after = "Pulong sa Huwebes 🗓️\nPaksa: badyet ng proyekto\n";
    expect(changed(diffLines(before, after))).toEqual([
      { kind: "removed", text: "Pulong sa Miyerkules 🗓️" },
      { kind: "added", text: "Pulong sa Huwebes 🗓️" },
    ]);
  });

  it("reports a final line break that was added or removed", () => {
    const diff = diffLines("one\ntwo", "one\ntwo\n");
    if (diff.tooLarge) throw new Error("expected a diff");
    const lines = diff.hunks.flatMap((hunk) => hunk.lines);
    expect(lines.filter((line) => line.kind !== "same")).toEqual([
      { kind: "removed", text: "two", ending: "", beforeLine: 2 },
      { kind: "added", text: "two", ending: "\n", afterLine: 2 },
    ]);
  });

  it("reports a line whose only change is its line ending", () => {
    expect(changed(diffLines("a\r\nb\r\n", "a\r\nb\n"))).toEqual([
      { kind: "removed", text: "b" },
      { kind: "added", text: "b" },
    ]);
  });

  it("splits separate changes into separate hunks", () => {
    const before = Array.from({ length: 20 }, (_, i) => `line ${i + 1}`).join(
      "\n",
    );
    const after = before
      .replace("line 2\n", "line two\n")
      .replace("line 18", "line eighteen");
    const diff = diffLines(before, after, { context: 2 });
    if (diff.tooLarge) throw new Error("expected a diff");
    expect(diff.hunks).toHaveLength(2);
    expect(diff.hunks[1].lines[0]).toMatchObject({
      text: "line 16",
      beforeLine: 16,
    });
  });

  it("gives up instead of approximating when the change is too large", () => {
    const before = Array.from({ length: 200 }, (_, i) => `a${i}`).join("\n");
    const after = Array.from({ length: 200 }, (_, i) => `b${i}`).join("\n");
    expect(diffLines(before, after, { maxCells: 10_000 })).toEqual({
      tooLarge: true,
    });
    // Unchanged lines at either end don't count against the bound.
    const long = `${before}\n${"same\n".repeat(5000)}`;
    expect(
      diffLines(long, long.replace("a100", "c100"), { maxCells: 10 }).tooLarge,
    ).toBe(false);
  });

  it("splits lines without losing a byte", () => {
    for (const text of ["", "a", "a\n", "a\r\nb\rc\n\nd", "\n\n"]) {
      const lines = splitLines(text);
      expect(lines.map((line) => line.text + line.ending).join("")).toBe(text);
    }
  });
});

describe("restoring line endings after a textarea", () => {
  const textarea = (text: string) => text.replace(/\r\n?/g, "\n");

  it("leaves files that use \\n alone", () => {
    expect(restoreLineEndings("a\nb\n", "a\nc\n")).toBe("a\nc\n");
  });

  it("gives a CRLF file its CRLF endings back, including on new lines", () => {
    const original = "Deadline: October 20\r\nOwner: Niña\r\n";
    const edited = textarea(original).replace("20", "23") + "Bagong linya\n";
    expect(restoreLineEndings(original, edited)).toBe(
      "Deadline: October 23\r\nOwner: Niña\r\nBagong linya\r\n",
    );
  });

  it("returns the original exactly when nothing was edited", () => {
    const mixed = "a\r\nb\nc\rd";
    expect(restoreLineEndings(mixed, textarea(mixed))).toBe(mixed);
  });

  it("keeps each unchanged line's own ending in a mixed file", () => {
    const original = "one\r\ntwo\nthree\r\nfour";
    const edited = "one\nTWO\nthree\nfour";
    expect(restoreLineEndings(original, edited)).toBe(
      "one\r\nTWO\r\nthree\r\nfour",
    );
    expect(restoreLineEndings("a\nb\nc\r\n", "a\nb\nC\n")).toBe("a\nb\nC\n");
  });

  it("respects a missing or added final line break", () => {
    expect(restoreLineEndings("a\r\nb", "a\nb\nc")).toBe("a\r\nb\r\nc");
    expect(restoreLineEndings("a\r\nb\r\n", "a\nb")).toBe("a\r\nb");
  });

  it("only changes what the user changed, so the preview shows just that", () => {
    const original = "Linya 1\r\nPetsa: Oktubre 20\r\nLinya 3\r\n";
    const after = restoreLineEndings(
      original,
      textarea(original).replace("Oktubre 20", "Oktubre 23"),
    );
    expect(changed(diffLines(original, after))).toEqual([
      { kind: "removed", text: "Petsa: Oktubre 20" },
      { kind: "added", text: "Petsa: Oktubre 23" },
    ]);
  });
});

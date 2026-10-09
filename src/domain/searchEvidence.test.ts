import { describe, expect, it } from "vitest";
import type { DocumentRecord, SearchResult } from "./contracts";
import {
  highlightSegments,
  matchLabel,
  mergeFolderResults,
} from "./searchEvidence";

const doc = (id: string) => ({ id, name: `${id}.md` }) as DocumentRecord;
const result = (
  document: DocumentRecord,
  method: SearchResult["method"] = "keyword",
  passages = 1,
) =>
  ({
    document,
    method,
    score: 1,
    passages: Array.from({ length: passages }, () => ({ text: "x" })),
  }) as SearchResult;
const hits = (text: string, query: string) =>
  highlightSegments(text, query)
    .filter((segment) => segment.hit)
    .map((segment) => segment.text);

describe("folder search results", () => {
  const plan = doc("plan");
  const notes = doc("notes");
  const gone = doc("gone");

  it("puts text matches first, then name-only matches, without repeats", () => {
    const merged = mergeFolderResults(
      [plan, notes],
      [result(notes)],
      [result(plan, "keyword", 0), result(notes, "keyword", 0)],
    );
    expect(merged.map((item) => item.document.id)).toEqual(["notes", "plan"]);
    expect(merged[0].passages).toHaveLength(1);
  });

  it("drops index results for files the folder no longer lists", () => {
    expect(mergeFolderResults([plan], [result(gone)], [])).toEqual([]);
  });

  it("binds index results to the folder's current record", () => {
    const current = { ...plan, sizeBytes: 9 } as DocumentRecord;
    const [merged] = mergeFolderResults([current], [result(plan)], []);
    expect(merged.document).toBe(current);
  });
});

describe("match labels", () => {
  it("never calls a keyword match semantic", () => {
    expect(matchLabel(result(plan()))).toBe("Words in the text");
    expect(matchLabel(result(plan(), "keyword", 0))).toBe("Words in the name");
    expect(matchLabel(result(plan(), "semantic"))).toBe("Similar meaning");
    expect(matchLabel(result(plan(), "hybrid"))).toBe("Words and meaning");
  });
  function plan() {
    return doc("plan");
  }
});

describe("excerpt highlighting", () => {
  it("marks whole words that start with a query word, in any case", () => {
    expect(hits("Planning the PLAN; an explanation.", "plan")).toEqual([
      "Planning",
      "PLAN",
    ]);
  });

  it("ignores accents like the index does, in Filipino text", () => {
    expect(
      hits("Si Niño ang bahala sa pagsasanay.", "nino pagsasanay"),
    ).toEqual(["Niño", "pagsasanay"]);
    expect(hits("Ang deadline ay sa Oktubre.", "OKTUBRE")).toEqual(["Oktubre"]);
  });

  it("keeps the excerpt's exact text, including accents", () => {
    const text = "Bayad sa café: ₱120";
    expect(
      highlightSegments(text, "cafe")
        .map((segment) => segment.text)
        .join(""),
    ).toBe(text);
  });

  it("marks nothing for an empty query or a missing word", () => {
    expect(hits("Ang listahan", "")).toEqual([]);
    expect(hits("Ang listahan", "budget")).toEqual([]);
  });
});

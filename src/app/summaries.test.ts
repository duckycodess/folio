import { describe, expect, it } from "vitest";
import type { GroundedResult, SourcePassage } from "../domain/contracts";
import {
  coveredPercent,
  createSummaryStore,
  isStale,
  summaryMarkdown,
  summaryPath,
} from "./summaries";

const ID = "w:notes/plan.md";
const HASH = `sha256:${"a".repeat(64)}`;

function passage(start: number, text: string, hash = HASH): SourcePassage {
  return {
    documentId: ID,
    documentContentHash: hash,
    offsetUnit: "utf8Byte",
    start,
    end: start + text.length,
    text,
  };
}

const DEADLINE = passage(10, "Deadline: October 23");
const OWNER = passage(40, "Maya writes the report");

function result(overrides: Partial<GroundedResult> = {}): GroundedResult {
  return {
    kind: "fileSummary",
    text: "",
    sources: [DEADLINE, OWNER],
    coverage: [ID],
    modelId: "qwen3-0.6b-q4-k-m",
    revision: "50968a4468ef4233ed78cd7c3de230dd1d61a56b",
    sentences: [
      { text: "The deadline is October 23.", citations: [DEADLINE] },
      { text: "Maya writes the report.", citations: [OWNER, DEADLINE] },
    ],
    coverageRanges: [
      {
        documentId: ID,
        documentContentHash: HASH,
        offsetUnit: "utf8Byte",
        ranges: [{ start: 0, end: 200 }],
        complete: true,
      },
    ],
    uncitedSentenceCount: 0,
    ...overrides,
  };
}

describe("summary store", () => {
  it("keeps one entry per file and reports the one running", () => {
    const store = createSummaryStore();
    store.set("a", { status: "running", startedAt: 1 });
    expect(store.running()).toBe("a");
    store.set("a", { status: "cancelled" });
    expect(store.running()).toBeNull();
    expect(store.get("a")).toEqual({ status: "cancelled" });
  });

  it("drops the oldest finished summaries past its limit, never a running one", () => {
    const store = createSummaryStore(2);
    store.set("running", { status: "running", startedAt: 1 });
    store.set("b", { status: "cancelled" });
    store.set("c", { status: "cancelled" });
    expect(store.get("running")).toBeDefined();
    expect(store.get("b")).toBeUndefined();
    expect(store.get("c")).toBeDefined();
  });

  it("never clears a running summary", () => {
    const store = createSummaryStore();
    store.set("a", { status: "running", startedAt: 1 });
    store.set("a", undefined);
    expect(store.running()).toBe("a");
  });

  it("tells subscribers about every change", () => {
    const store = createSummaryStore();
    let calls = 0;
    const stop = store.subscribe(() => calls++);
    store.set("a", { status: "cancelled" });
    stop();
    store.set("a", undefined);
    expect(calls).toBe(1);
    expect(store.version()).toBe(2);
  });
});

describe("summary checks", () => {
  it("is stale once the file has a different revision", () => {
    expect(isStale(result(), { id: ID, contentHash: HASH })).toBe(false);
    expect(
      isStale(result(), { id: ID, contentHash: `sha256:${"b".repeat(64)}` }),
    ).toBe(true);
    expect(isStale(result(), { id: ID, contentHash: undefined })).toBe(false);
  });

  it("reports how much of the file a partial summary read", () => {
    const text = "x".repeat(200);
    expect(coveredPercent(result(), { id: ID, content: text })).toBe(100);
    const partial = result({
      kind: "partialSummary",
      coverageRanges: [
        {
          documentId: ID,
          documentContentHash: HASH,
          offsetUnit: "utf8Byte",
          ranges: [
            { start: 0, end: 50 },
            { start: 100, end: 130 },
          ],
          complete: false,
        },
      ],
    });
    expect(coveredPercent(partial, { id: ID, content: text })).toBe(40);
    expect(coveredPercent(partial, { id: "other", content: text })).toBeNull();
    // Measured against the extracted text, not the file on disk: a PDF's
    // file size would understate what was read.
    expect(coveredPercent(partial, { id: ID, content: "é".repeat(100) })).toBe(
      40,
    );
    expect(coveredPercent(partial, { id: ID, content: undefined })).toBeNull();
  });
});

describe("saving a summary", () => {
  it("names the file after its source, without taking an existing name", () => {
    expect(summaryPath("notes/plan.md", [])).toBe("notes/plan summary.md");
    expect(summaryPath("plan.txt", ["Plan Summary.md"])).toBe(
      "plan summary 2.md",
    );
    expect(summaryPath("README", [])).toBe("README summary.md");
  });

  it("writes each point with numbered sources, marked as generated", () => {
    const text = summaryMarkdown(
      { name: "plan.md", relativePath: "notes/plan.md" },
      result(),
      new Date(2026, 9, 10, 12),
    );
    expect(text).toContain("# Summary of plan.md");
    expect(text).toContain(
      "qwen3-0.6b-q4-k-m (revision 50968a4468ef) on 2026-10-10",
    );
    expect(text).toContain("Not reviewed for accuracy");
    expect(text).toContain("- The deadline is October 23. [1]");
    expect(text).toContain("- Maya writes the report. [2][1]");
    expect(text).toContain("1. notes/plan.md: “Deadline: October 23”");
    expect(text).toContain("2. notes/plan.md: “Maya writes the report”");
  });

  it("calls a partial summary partial", () => {
    expect(
      summaryMarkdown(
        { name: "plan.md", relativePath: "plan.md" },
        result({ kind: "partialSummary" }),
        new Date(0),
      ),
    ).toContain("# Partial summary of plan.md");
  });
});

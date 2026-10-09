import { describe, expect, it } from "vitest";
import type { HistoryEntry } from "./contracts";
import { batchTitle, changeKind, groupHistory } from "./activity";

let n = 0;
const entry = (overrides: Partial<HistoryEntry>): HistoryEntry => ({
  id: `h${++n}`,
  planId: "p1",
  operationIndex: 0,
  appliedAt: 1000,
  beforeRelativePath: "Downloads/a.md",
  afterRelativePath: "Research/a.md",
  afterContentHash: "sha256:x" as HistoryEntry["afterContentHash"],
  recoverable: true,
  ...overrides,
});

describe("change kinds", () => {
  it("tells moves, renames, edits, creates and deletes apart", () => {
    expect(changeKind(entry({}))).toBe("move");
    expect(changeKind(entry({ afterRelativePath: "Downloads/b.md" }))).toBe(
      "rename",
    );
    expect(changeKind(entry({ afterRelativePath: "Downloads/a.md" }))).toBe(
      "edit",
    );
    expect(changeKind(entry({ beforeRelativePath: undefined }))).toBe("create");
    expect(changeKind(entry({ afterRelativePath: undefined }))).toBe("delete");
    expect(
      changeKind(
        entry({ beforeRelativePath: "a.md", afterRelativePath: "b.md" }),
      ),
    ).toBe("rename");
  });
});

describe("activity batches", () => {
  it("groups one plan's operations into one entry, newest plan first", () => {
    const batches = groupHistory([
      entry({ planId: "old", appliedAt: 100 }),
      entry({ planId: "new", appliedAt: 900, operationIndex: 1 }),
      entry({ planId: "new", appliedAt: 900, operationIndex: 0 }),
      entry({ planId: "new", appliedAt: 901, operationIndex: 2 }),
    ]);
    expect(batches.map((batch) => batch.planId)).toEqual(["new", "old"]);
    expect(batches[0].entries.map((row) => row.operationIndex)).toEqual([
      0, 1, 2,
    ]);
    expect(batchTitle(batches[0])).toBe("Moved 3 files");
    expect(batchTitle(batches[1])).toBe("Moved 1 file");
  });

  it("names a mixed batch without claiming one kind", () => {
    const [batch] = groupHistory([
      entry({}),
      entry({ operationIndex: 1, afterRelativePath: "Downloads/b.md" }),
    ]);
    expect(batchTitle(batch)).toBe("Changed 2 files");
  });

  it("reports undone and partly undone batches", () => {
    const [partly] = groupHistory([
      entry({ undoneAt: 2000 }),
      entry({ operationIndex: 1 }),
    ]);
    expect(partly.status).toBe("partlyUndone");
    expect(partly.canUndo).toBe(true);
    const [undone] = groupHistory([entry({ planId: "u", undoneAt: 2000 })]);
    expect(undone.status).toBe("undone");
    expect(undone.canUndo).toBe(false);
  });

  it("never offers Undo for changes whose earlier version wasn't kept", () => {
    const [batch] = groupHistory([entry({ recoverable: false })]);
    expect(batch.canUndo).toBe(false);
  });

  it("has nothing to show when nothing was recorded", () => {
    expect(groupHistory([])).toEqual([]);
  });
});

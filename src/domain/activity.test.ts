import { describe, expect, it } from "vitest";
import type {
  ActivityBatch as RecordedBatch,
  ActivityOperation,
  HistoryEntry,
} from "./contracts";
import {
  batchTitle,
  changeKind,
  fromActivity,
  SOURCE_LABELS,
  stopSummary,
} from "./activity";

let n = 0;
const entry = (overrides: Partial<HistoryEntry>): HistoryEntry => ({
  id: `h${++n}`,
  planId: "p1",
  operationIndex: 0,
  operationKind: "move",
  appliedAt: 1000,
  beforeRelativePath: "Downloads/a.md",
  afterRelativePath: "Research/a.md",
  afterContentHash: "sha256:x" as HistoryEntry["afterContentHash"],
  recoverable: true,
  ...overrides,
});

/** A move that changed its file, with its history. */
const moved = (index: number, overrides: Partial<HistoryEntry> = {}) =>
  ({
    operationIndex: index,
    operationKind: "move",
    beforeRelativePath: `Downloads/${index}.md`,
    afterRelativePath: `Research/${index}.md`,
    status: "succeeded",
    history: entry({
      operationIndex: index,
      beforeRelativePath: `Downloads/${index}.md`,
      afterRelativePath: `Research/${index}.md`,
      ...overrides,
    }),
  }) satisfies ActivityOperation;

/** A move that changed nothing, for the reason given. */
const unmoved = (
  index: number,
  status?: ActivityOperation["status"],
  message?: string,
) =>
  ({
    operationIndex: index,
    operationKind: "move",
    beforeRelativePath: `Downloads/${index}.md`,
    afterRelativePath: `Research/${index}.md`,
    status,
    error: message
      ? { code: "targetChanged", message, details: {} }
      : undefined,
  }) as ActivityOperation;

const batch = (
  operations: ActivityOperation[],
  overrides: Partial<RecordedBatch> = {},
): RecordedBatch => ({
  planId: "p1",
  source: "organize",
  appliedAt: 1000,
  finishedAt: 1001,
  stopReason: "completed",
  operations,
  ...overrides,
});

const one = (recorded: RecordedBatch) => fromActivity([recorded])[0];

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
  it("keeps the native order and every operation of each plan", () => {
    const batches = fromActivity([
      batch([moved(0), moved(1), moved(2)], { planId: "new", appliedAt: 900 }),
      batch([moved(0)], { planId: "old", appliedAt: 100 }),
    ]);
    expect(batches.map((item) => item.planId)).toEqual(["new", "old"]);
    expect(batchTitle(batches[0])).toBe("Moved 3 files");
    expect(batchTitle(batches[1])).toBe("Moved 1 file");
    expect(batches[0].status).toBe("applied");
    expect(stopSummary(batches[0])).toBeNull();
  });

  it("names a mixed batch without claiming one kind", () => {
    const mixed = one(
      batch([moved(0), { ...moved(1), operationKind: "rename" }]),
    );
    expect(batchTitle(mixed)).toBe("Changed 2 files");
  });

  it("says where the change was started, and nothing when it wasn't recorded", () => {
    expect(
      SOURCE_LABELS[one(batch([moved(0)], { source: "graph" })).source],
    ).toBe("From Graph");
    expect(
      SOURCE_LABELS[one(batch([moved(0)], { source: "assistant" })).source],
    ).toBe("From Ask & Act");
    expect(
      SOURCE_LABELS[one(batch([moved(0)], { source: "unknown" })).source],
    ).toBeNull();
  });

  it("reports undone and partly undone batches", () => {
    const partly = one(batch([moved(0, { undoneAt: 2000 }), moved(1)]));
    expect(partly.status).toBe("partlyUndone");
    expect(partly.canUndo).toBe(true);
    const undone = one(batch([moved(0, { undoneAt: 2000 })]));
    expect(undone.status).toBe("undone");
    expect(undone.canUndo).toBe(false);
  });

  it("never offers Undo for changes whose earlier version wasn't kept", () => {
    expect(one(batch([moved(0, { recoverable: false })])).canUndo).toBe(false);
  });
});

describe("failed and cancelled batches", () => {
  it("shows a batch that stopped partway, and keeps what changed", () => {
    const stopped = one(
      batch(
        [
          moved(0),
          unmoved(1, "failed", "This file changed since the preview."),
          unmoved(2, "notStarted"),
        ],
        { stopReason: "failed" },
      ),
    );
    expect(stopped.status).toBe("stopped");
    expect(batchTitle(stopped)).toBe("Moved 1 of 3 files");
    expect(stopSummary(stopped)).toBe(
      "Stopped at Research/1.md: This file changed since the preview. The earlier change was kept.",
    );
    expect(stopped.entries).toHaveLength(1);
    expect(stopped.canUndo).toBe(true);
  });

  it("says plainly when nothing was changed", () => {
    const failed = one(
      batch([unmoved(0, "failed", "The folder is no longer available.")], {
        stopReason: "failed",
      }),
    );
    expect(failed.status).toBe("nothingChanged");
    expect(batchTitle(failed)).toBe("Couldn't move 1 file");
    expect(stopSummary(failed)).toBe(
      "Stopped at Research/0.md: The folder is no longer available. Nothing was changed.",
    );
    expect(failed.canUndo).toBe(false);
  });

  it("shows a cancelled batch with what ran before the cancel", () => {
    const cancelled = one(
      batch([moved(0), unmoved(1, "cancelled")], { stopReason: "cancelled" }),
    );
    expect(cancelled.status).toBe("stopped");
    expect(stopSummary(cancelled)).toBe(
      "Cancelled after 1 of 2 changes. The rest weren't started.",
    );
  });

  it("never guesses an outcome Folio didn't record", () => {
    // Recorded before outcomes were kept: one change has history, one doesn't.
    const earlier = one(
      batch([moved(0), unmoved(1)], {
        source: "unknown",
        finishedAt: undefined,
        stopReason: "failed",
      }),
    );
    expect(earlier.operations[1].status).toBeUndefined();
    expect(stopSummary(earlier)).toBe(
      "Folio didn't record what happened to 1 file in this change.",
    );
  });
});

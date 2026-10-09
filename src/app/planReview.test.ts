import { describe, expect, it } from "vitest";
import type {
  ActionPlan,
  ApplyReport,
  FileOperation,
  ImpactCandidate,
  OperationOutcome,
  UndoReport,
} from "../domain/contracts";
import {
  impactGroups,
  impactKind,
  impactProvenance,
  planRow,
  summarizeApply,
  summarizeUndo,
  undoBlockers,
} from "./planReview";

function rename(from: string, to: string): FileOperation {
  return {
    kind: "rename",
    documentId: `w:${from}`,
    relativePath: from,
    expectedContentHash: "a".repeat(64),
    destinationRelativePath: to,
    expectedDestination: "absent",
  };
}

const PLAN: ActionPlan = {
  id: "plan-1",
  workspaceId: "w",
  createdAt: 1,
  expiresAt: 2,
  operations: [
    rename("notes/a.md", "notes/2026-a.md"),
    rename("notes/b.md", "notes/2026-b.md"),
    rename("notes/c.md", "notes/2026-c.md"),
  ],
  impacts: [],
  digest: "d".repeat(64),
};

function report(
  statuses: OperationOutcome["status"][],
  overrides: Partial<ApplyReport> = {},
  stopReason: ApplyReport["batch"]["stopReason"] = "completed",
): ApplyReport {
  return {
    batch: {
      planId: PLAN.id,
      planDigest: PLAN.digest,
      startedAt: 1,
      finishedAt: 2,
      stopReason,
      outcomes: statuses.map((status, operationIndex) => ({
        operationIndex,
        status,
        ...(status === "failed"
          ? {
              error: {
                code: "destinationExists",
                message: "A file already has that name.",
              },
            }
          : {}),
      })),
    },
    historySettled: true,
    indexRefreshed: true,
    ...overrides,
  };
}

describe("exact preview rows", () => {
  it("shows every rename as from → to", () => {
    expect(PLAN.operations.map(planRow)).toEqual([
      { action: "Rename", from: "notes/a.md", to: "notes/2026-a.md" },
      { action: "Rename", from: "notes/b.md", to: "notes/2026-b.md" },
      { action: "Rename", from: "notes/c.md", to: "notes/2026-c.md" },
    ]);
  });

  it("names a deleted file and calls the result a deletion", () => {
    const deletion: ActionPlan = {
      ...PLAN,
      operations: [
        {
          kind: "delete",
          documentId: "w:notes/a.md",
          relativePath: "notes/a.md",
          expectedContentHash: "a".repeat(64),
        },
      ],
    };
    expect(deletion.operations.map(planRow)).toEqual([
      { action: "Delete", from: "notes/a.md" },
    ]);
    expect(summarizeApply(deletion, report(["succeeded"])).headline).toBe(
      "Deleted 1 file.",
    );
  });
});

describe("what an applied plan reports", () => {
  it("calls it saved only when every change succeeded and the index caught up", () => {
    expect(
      summarizeApply(PLAN, report(["succeeded", "succeeded", "succeeded"]))
        .tone,
    ).toBe("saved");
    const lagging = summarizeApply(
      PLAN,
      report(["succeeded", "succeeded", "succeeded"], {
        indexRefreshed: false,
      }),
    );
    expect(lagging.tone).toBe("partial");
    expect(lagging.details.join(" ")).toMatch(/hasn't caught up/);
  });

  it("never says nothing changed when a batch stopped partway", () => {
    const summary = summarizeApply(
      PLAN,
      report(["succeeded", "failed", "notStarted"], {}, "failed"),
    );
    expect(summary.tone).toBe("partial");
    expect(summary.headline).toMatch(
      /^Saved 1 of 3 changes\. Stopped at notes\/b\.md/,
    );
    expect(`${summary.headline} ${summary.details.join(" ")}`).not.toMatch(
      /No file was changed|didn't change anything/i,
    );
    expect(summary.details.join(" ")).toMatch(/Earlier changes were kept/);
    expect(summary.undoable).toBe(true);
  });

  it("says nothing changed only when no operation succeeded", () => {
    const summary = summarizeApply(
      PLAN,
      report(["failed", "notStarted", "notStarted"], {}, "failed"),
    );
    expect(summary.tone).toBe("nothingSaved");
    expect(summary.headline).toMatch(/^No file was changed\./);
    expect(summary.undoable).toBe(false);
  });

  it("warns when Undo history couldn't be recorded", () => {
    const summary = summarizeApply(
      PLAN,
      report(["succeeded", "succeeded", "succeeded"], {
        historySettled: false,
      }),
    );
    expect(summary.tone).toBe("saved");
    expect(summary.details.join(" ")).toMatch(/Undo may not be available/);
  });

  it("reports a cancelled batch by what it kept", () => {
    const summary = summarizeApply(
      PLAN,
      report(["succeeded", "cancelled", "notStarted"], {}, "cancelled"),
    );
    expect(summary.headline).toBe("Stopped after 1 of 3 changes.");
  });
});

describe("Undo", () => {
  const base: UndoReport = {
    planId: PLAN.id,
    undoneEntryIds: [],
    remainingEntryIds: [],
    indexRefreshed: true,
  };

  it("never claims a partial Undo reversed nothing", () => {
    const partial = summarizeUndo({
      ...base,
      undoneEntryIds: ["h1"],
      remainingEntryIds: ["h2"],
      error: { code: "undoConflict", message: "changed" },
    });
    expect(partial.complete).toBe(false);
    expect(partial.headline).toMatch(/^Undid 1 of 2 changes, then stopped/);
  });

  it("says when everything was undone, or nothing", () => {
    expect(summarizeUndo({ ...base, undoneEntryIds: ["h1", "h2"] })).toEqual({
      complete: true,
      headline: "Undid 2 changes.",
    });
    expect(
      summarizeUndo({
        ...base,
        remainingEntryIds: ["h1"],
        error: { code: "undoConflict", message: "changed" },
      }).headline,
    ).toMatch(/^Nothing was undone\./);
  });

  it("explains each blocked file", () => {
    expect(
      undoBlockers({
        planId: PLAN.id,
        entryIds: ["h1"],
        undoable: false,
        conflicts: [
          {
            historyEntryId: "h1",
            relativePath: "notes/2026-a.md",
            observedContentHash: null,
            reason: "missing",
          },
        ],
      }),
    ).toEqual(["notes/2026-a.md is no longer there."]);
  });
});

describe("a change saved without its Undo record", () => {
  // The native core reports `historyRequired` only after the file changed.
  function afterWrite(
    statuses: OperationOutcome["status"][],
    failedAt: number,
  ): ApplyReport {
    const base = report(statuses, {}, "failed");
    return {
      ...base,
      batch: {
        ...base.batch,
        outcomes: base.batch.outcomes.map((outcome) =>
          outcome.operationIndex === failedAt
            ? {
                ...outcome,
                error: {
                  code: "historyRequired",
                  message:
                    "The file was changed, but Folio could not record how to undo it.",
                },
              }
            : outcome,
        ),
      },
    };
  }

  it("never says nothing changed when the only failure followed a write", () => {
    const summary = summarizeApply(
      PLAN,
      afterWrite(["failed", "notStarted", "notStarted"], 0),
    );
    expect(summary.tone).toBe("partial");
    expect(`${summary.headline} ${summary.details.join(" ")}`).not.toMatch(
      /No file was changed/,
    );
    expect(summary.headline).toMatch(/^Saved 1 of 3 changes\./);
    expect(summary.details.join(" ")).toMatch(/Keep a copy/);
    // Nothing that changed can be undone: its history wasn't recorded.
    expect(summary.undoable).toBe(false);
  });

  it("counts it with the earlier changes, which stay undoable", () => {
    const summary = summarizeApply(
      PLAN,
      afterWrite(["succeeded", "failed", "notStarted"], 1),
    );
    expect(summary.headline).toMatch(/^Saved 2 of 3 changes\./);
    expect(summary.details.join(" ")).toMatch(/Earlier changes were kept/);
    expect(summary.undoable).toBe(true);
  });
});

describe("Ripple candidates", () => {
  function impact(
    relativePath: string,
    fields: Partial<ImpactCandidate> = {},
  ): ImpactCandidate {
    return {
      documentId: `w:${relativePath}`,
      relativePath,
      reason: "Mentions “October 20”.",
      evidence: [],
      strength: "evidence",
      ...fields,
    };
  }

  const LINK = impact("notes/links-here.md", {
    relationshipType: "explicitReference",
    provenance: "documentLink",
  });
  const COPY = impact("backup/plan.md", { strength: "similarityOnly" });
  const SIMILAR = impact("notes/similar.md", {
    strength: "similarityOnly",
    relationshipType: "similarity",
    provenance: "embedding",
  });
  const SHARED_FACT = impact("notes/same-fact.md", {
    relationshipType: "sharedFactCandidate",
    provenance: "model",
  });
  const UNSAID = impact("notes/unsaid.md");

  it("groups candidates by how Folio knows they're related", () => {
    expect(impactGroups([SIMILAR, LINK, COPY, SHARED_FACT, UNSAID])).toEqual({
      links: [LINK],
      copies: [COPY],
      inferred: [SIMILAR, SHARED_FACT],
      other: [UNSAID],
    });
  });

  it("never labels a link or an identical copy as AI", () => {
    for (const candidate of [LINK, COPY]) {
      expect(impactKind(candidate)).not.toBe("inferred");
      expect(impactProvenance(candidate).ai).toBe(false);
      expect(impactProvenance(candidate).label).not.toMatch(/AI|model/);
    }
    // A link is a link, whichever field says so.
    expect(impactKind(impact("a.md", { provenance: "documentLink" }))).toBe(
      "links",
    );
    expect(impactProvenance(SHARED_FACT)).toMatchObject({ ai: true });
    expect(impactProvenance(SIMILAR)).toMatchObject({ ai: true });
  });

  it("doesn't call a candidate a copy unless it's similarity-only and names no relationship", () => {
    expect(impactKind(UNSAID)).toBe("other");
    expect(impactProvenance(UNSAID).ai).toBe(false);
  });
});

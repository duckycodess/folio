import { describe, expect, it } from "vitest";
import {
  assertApprovalMatches,
  assertUndoable,
  canonicalPlanBytes,
  planDigest,
  preflightPlan,
  preflightUndo,
  settleBatch,
  undoPaths,
  verifyPlanDigest,
  type AttemptOutcome,
} from "./plan";
import { isFolioError } from "./errors";
import { hashText } from "./hash";
import {
  ABSENT,
  deleteOperation,
  deletionEntry,
  documentId,
  editOperation,
  historyEntry,
  makePlan,
  observedPaths,
  renameOperation,
  WORKSPACE,
} from "./test-support";
import type { Approval, FolioErrorCode } from "./contracts";

const PLAN_TEXT = "Deadline: October 20\n";
const EDITED_TEXT = "Deadline: October 23\n";

function codeOf(run: () => unknown): FolioErrorCode | string {
  try {
    run();
  } catch (cause) {
    return isFolioError(cause) ? cause.code : `not-a-folio-error: ${cause}`;
  }
  return "no-error";
}

async function codeOfAsync(
  run: () => Promise<unknown>,
): Promise<FolioErrorCode | string> {
  try {
    await run();
  } catch (cause) {
    return isFolioError(cause) ? cause.code : `not-a-folio-error: ${cause}`;
  }
  return "no-error";
}

async function deadlinePlan() {
  return makePlan({
    id: "plan-1",
    operations: [
      await editOperation("projects/project-plan.md", PLAN_TEXT, EDITED_TEXT),
    ],
  });
}

describe("plan preflight", () => {
  it("refuses an operation that would leave the authorized folder", async () => {
    const plan = await makePlan({
      id: "plan-escape",
      operations: [
        {
          kind: "edit",
          documentId: `${WORKSPACE}:x`,
          relativePath: "../outside.md",
          expectedContentHash: await hashText("private"),
          after: "changed",
        },
      ],
    });
    expect(codeOf(() => preflightPlan(plan, {}, 1_500))).toBe(
      "pathEscapesWorkspace",
    );
  });

  it("refuses the whole batch when any later target already changed", async () => {
    const plan = await makePlan({
      id: "plan-batch",
      operations: [
        await editOperation("projects/project-plan.md", PLAN_TEXT, EDITED_TEXT),
        await editOperation("notes/paalala.md", "Paalala\n", "Paalala 23\n"),
      ],
    });
    const observed = await observedPaths({
      "projects/project-plan.md": PLAN_TEXT,
      "notes/paalala.md": "Binago ng ibang app\n",
    });
    // The first operation is applicable, but preflight covers every target
    // before anything is written, so nothing may start.
    expect(codeOf(() => preflightPlan(plan, observed, 1_500))).toBe(
      "targetChanged",
    );
  });

  it("refuses a rename onto an existing file and leaves it alone", async () => {
    const plan = await makePlan({
      id: "plan-rename",
      operations: [
        await renameOperation(
          "notes/paalala.md",
          "Paalala\n",
          "notes/tala-sa-proyekto.md",
        ),
      ],
    });
    const observed = await observedPaths({
      "notes/paalala.md": "Paalala\n",
      "notes/tala-sa-proyekto.md": "Ibang tala\n",
    });
    expect(codeOf(() => preflightPlan(plan, observed, 1_500))).toBe(
      "destinationExists",
    );
    // The occupied destination was never part of a write decision.
    expect(observed["notes/tala-sa-proyekto.md"].contentHash).toBe(
      await hashText("Ibang tala\n"),
    );
  });

  it("refuses two operations acting on the same file", async () => {
    const plan = await makePlan({
      id: "plan-duplicate",
      operations: [
        await editOperation("projects/project-plan.md", PLAN_TEXT, EDITED_TEXT),
        await renameOperation(
          "projects/project-plan.md",
          PLAN_TEXT,
          "projects/plano.md",
        ),
      ],
    });
    const observed = await observedPaths({
      "projects/project-plan.md": PLAN_TEXT,
      "projects/plano.md": null,
    });
    expect(codeOf(() => preflightPlan(plan, observed, 1_500))).toBe(
      "duplicateOperationTarget",
    );
  });

  it("refuses two operations whose names differ only in case", async () => {
    // On Windows and macOS these are one file, so the batch would act on the
    // same document twice.
    const plan = await makePlan({
      id: "plan-case",
      operations: [
        await editOperation("projects/Project-Plan.md", PLAN_TEXT, EDITED_TEXT),
        await renameOperation(
          "projects/project-plan.md",
          PLAN_TEXT,
          "projects/plano.md",
        ),
      ],
    });
    const observed = await observedPaths({
      "projects/Project-Plan.md": PLAN_TEXT,
      "projects/project-plan.md": PLAN_TEXT,
      "projects/plano.md": null,
    });
    expect(codeOf(() => preflightPlan(plan, observed, 1_500))).toBe(
      "duplicateOperationTarget",
    );
  });

  it("refuses a rename whose destination differs from the source only in case", async () => {
    const plan = await makePlan({
      id: "plan-case-rename",
      operations: [
        await renameOperation(
          "notes/paalala.md",
          "Paalala\n",
          "notes/Paalala.md",
        ),
      ],
    });
    const observed = await observedPaths({
      "notes/paalala.md": "Paalala\n",
      "notes/Paalala.md": null,
    });
    expect(codeOf(() => preflightPlan(plan, observed, 1_500))).toBe(
      "operationUnsupported",
    );
  });

  it("refuses a target that exists but is not a file", async () => {
    const plan = await deadlinePlan();
    const observed = {
      "projects/project-plan.md": {
        exists: true,
        isFile: false,
        contentHash: null,
      },
    };
    expect(codeOf(() => preflightPlan(plan, observed, 1_500))).toBe(
      "operationUnsupported",
    );
  });

  it("looks the target up under its normalized path", async () => {
    // The operation carries a decomposed Filipino filename; the observed state
    // is keyed by the composed one, which is the same file.
    const decomposed = "courses/pagsasanay-n\u0303.md";
    const composed = "courses/pagsasanay-ñ.md";
    const plan = await makePlan({
      id: "plan-nfc",
      operations: [await editOperation(decomposed, PLAN_TEXT, EDITED_TEXT)],
    });
    const observed = await observedPaths({ [composed]: PLAN_TEXT });
    expect(codeOf(() => preflightPlan(plan, observed, 1_500))).toBe("no-error");
  });

  it("refuses to edit a format Folio only reads", async () => {
    const plan = await makePlan({
      id: "plan-pdf",
      operations: [
        await editOperation("research/paper.pdf", "%PDF", "%PDF edited"),
      ],
    });
    expect(codeOf(() => preflightPlan(plan, {}, 1_500))).toBe(
      "unsupportedMediaType",
    );
  });

  it("refuses to delete a format Folio only reads", async () => {
    const plan = await makePlan({
      id: "plan-pdf-delete",
      operations: [await deleteOperation("research/paper.pdf", "%PDF")],
    });
    expect(codeOf(() => preflightPlan(plan, {}, 1_500))).toBe(
      "unsupportedMediaType",
    );
  });

  it("checks a deletion's target and nothing else", async () => {
    const plan = await makePlan({
      id: "plan-delete",
      operations: [await deleteOperation("notes/paalala.md", "Paalala\n")],
    });
    const current = await observedPaths({ "notes/paalala.md": "Paalala\n" });
    expect(codeOf(() => preflightPlan(plan, current, 1_500))).toBe("no-error");
    const changed = await observedPaths({ "notes/paalala.md": "Binago\n" });
    expect(codeOf(() => preflightPlan(plan, changed, 1_500))).toBe(
      "targetChanged",
    );
  });

  it("refuses a missing target", async () => {
    const plan = await deadlinePlan();
    const observed = await observedPaths({
      "projects/project-plan.md": null,
    });
    expect(codeOf(() => preflightPlan(plan, observed, 1_500))).toBe(
      "targetMissing",
    );
  });

  it("re-checks expiry at application time", async () => {
    const plan = await deadlinePlan();
    const observed = await observedPaths({
      "projects/project-plan.md": PLAN_TEXT,
    });
    expect(codeOf(() => preflightPlan(plan, observed, 1_500))).toBe("no-error");
    expect(codeOf(() => preflightPlan(plan, observed, 2_000))).toBe(
      "planExpired",
    );
  });
});

describe("plan digest", () => {
  it("changes when any operation changes", async () => {
    const first = await deadlinePlan();
    const second = await makePlan({
      id: "plan-1",
      operations: [
        await editOperation(
          "projects/project-plan.md",
          PLAN_TEXT,
          "Deadline: October 30\n",
        ),
      ],
    });
    expect(second.digest).not.toBe(first.digest);
  });

  it("cannot be forged by a field boundary inside a document body", async () => {
    const sneaky = await makePlan({
      id: "plan-1",
      operations: [
        await editOperation(
          "projects/project-plan.md",
          PLAN_TEXT,
          "24:projects/project-plan.md\n",
        ),
      ],
    });
    const plain = await makePlan({
      id: "plan-1",
      operations: [
        await editOperation("projects/project-plan.md", PLAN_TEXT, ""),
      ],
    });
    expect(sneaky.digest).not.toBe(plain.digest);
    expect(new TextDecoder().decode(canonicalPlanBytes(sneaky))).toContain(
      "28:24:projects/project-plan.md\n",
    );
  });

  it("rejects a plan edited after the native core issued it", async () => {
    const plan = await deadlinePlan();
    const tampered = {
      ...plan,
      operations: [{ ...plan.operations[0], after: "Deadline: never\n" }],
    };
    expect(await codeOfAsync(() => verifyPlanDigest(plan))).toBe("no-error");
    expect(await codeOfAsync(() => verifyPlanDigest(tampered))).toBe(
      "planDigestMismatch",
    );
  });

  it("refuses an approval whose plan changed under the same identity", async () => {
    const plan = await deadlinePlan();
    const approval: Approval = {
      planId: plan.id,
      planDigest: plan.digest,
      approvedAt: 1_200,
    };
    const changed = await makePlan({
      id: plan.id,
      operations: [
        await editOperation(
          "projects/project-plan.md",
          PLAN_TEXT,
          "Deadline: October 30\n",
        ),
      ],
    });
    expect(codeOf(() => assertApprovalMatches(plan, approval))).toBe(
      "no-error",
    );
    expect(codeOf(() => assertApprovalMatches(changed, approval))).toBe(
      "approvalStale",
    );
  });

  it("matches the digest an independent implementation computes", async () => {
    const plan = await deadlinePlan();
    expect(await planDigest(plan)).toBe(plan.digest);
  });
});

async function threeOperationPlan() {
  return makePlan({
    id: "plan-three",
    operations: [
      await editOperation("projects/project-plan.md", PLAN_TEXT, EDITED_TEXT),
      await editOperation("notes/paalala.md", "Paalala\n", "Paalala 23\n"),
      await editOperation("meetings/notes.md", "Notes\n", "Notes 23\n"),
    ],
  });
}

const succeeded = (id: string, at: number): AttemptOutcome => ({
  status: "succeeded",
  historyEntryId: id,
  completedAt: at,
});

describe("batch outcomes", () => {
  it("records one durable outcome per operation when everything succeeds", async () => {
    const plan = await threeOperationPlan();
    const approval: Approval = {
      planId: plan.id,
      planDigest: plan.digest,
      approvedAt: 1_100,
    };
    const result = settleBatch({
      plan,
      approval,
      attempts: [
        succeeded("h1", 1_201),
        succeeded("h2", 1_202),
        succeeded("h3", 1_203),
      ],
      startedAt: 1_200,
      finishedAt: 1_204,
    });
    expect(result.stopReason).toBe("completed");
    expect(result.outcomes.map((outcome) => outcome.status)).toEqual([
      "succeeded",
      "succeeded",
      "succeeded",
    ]);
    expect(result.outcomes.every((outcome) => outcome.historyEntryId)).toBe(
      true,
    );
  });

  it("stops at the first failure and keeps earlier work", async () => {
    const plan = await threeOperationPlan();
    const approval: Approval = {
      planId: plan.id,
      planDigest: plan.digest,
      approvedAt: 1_100,
    };
    const result = settleBatch({
      plan,
      approval,
      attempts: [
        succeeded("h1", 1_201),
        {
          status: "failed",
          error: { code: "internal", message: "The disk is full." },
          completedAt: 1_202,
        },
      ],
      startedAt: 1_200,
      finishedAt: 1_203,
    });
    expect(result.stopReason).toBe("failed");
    expect(result.outcomes.map((outcome) => outcome.status)).toEqual([
      "succeeded",
      "failed",
      "notStarted",
    ]);
    expect(result.outcomes[0].historyEntryId).toBe("h1");
    expect(result.outcomes[1].error?.code).toBe("internal");
    expect(result.outcomes[2].historyEntryId).toBeUndefined();
  });

  it("refuses a report that continued past a failure", async () => {
    const plan = await threeOperationPlan();
    const approval: Approval = {
      planId: plan.id,
      planDigest: plan.digest,
      approvedAt: 1_100,
    };
    expect(
      codeOf(() =>
        settleBatch({
          plan,
          approval,
          attempts: [
            {
              status: "failed",
              error: { code: "internal", message: "Write failed." },
              completedAt: 1_201,
            },
            succeeded("h2", 1_202),
          ],
          startedAt: 1_200,
          finishedAt: 1_203,
        }),
      ),
    ).toBe("planStateInvalid");
  });

  it("finishes the running operation on cancellation and starts no other", async () => {
    const plan = await threeOperationPlan();
    const approval: Approval = {
      planId: plan.id,
      planDigest: plan.digest,
      approvedAt: 1_100,
    };
    const result = settleBatch({
      plan,
      approval,
      attempts: [succeeded("h1", 1_201)],
      cancelledAfterIndex: 0,
      startedAt: 1_200,
      finishedAt: 1_202,
    });
    expect(result.stopReason).toBe("cancelled");
    expect(result.outcomes.map((outcome) => outcome.status)).toEqual([
      "succeeded",
      "cancelled",
      "cancelled",
    ]);
    // The completed change is retained; cancellation never rolls it back.
    expect(result.outcomes[0].historyEntryId).toBe("h1");
  });

  it("requires a history entry for every completed operation", async () => {
    const plan = await deadlinePlan();
    const approval: Approval = {
      planId: plan.id,
      planDigest: plan.digest,
      approvedAt: 1_100,
    };
    expect(
      codeOf(() =>
        settleBatch({
          plan,
          approval,
          attempts: [
            { status: "succeeded", historyEntryId: "  ", completedAt: 1_201 },
          ],
          startedAt: 1_200,
          finishedAt: 1_202,
        }),
      ),
    ).toBe("historyRequired");
  });

  it("refuses a cancellation reported before any operation ran", async () => {
    const plan = await threeOperationPlan();
    const approval: Approval = {
      planId: plan.id,
      planDigest: plan.digest,
      approvedAt: 1_100,
    };
    // Nothing began, so there is no finished operation to record.
    expect(
      codeOf(() =>
        settleBatch({
          plan,
          approval,
          attempts: [],
          cancelledAfterIndex: -1,
          startedAt: 1_200,
          finishedAt: 1_201,
        }),
      ),
    ).toBe("planStateInvalid");
  });

  it("refuses a short batch with no failure and no cancellation", async () => {
    const plan = await threeOperationPlan();
    const approval: Approval = {
      planId: plan.id,
      planDigest: plan.digest,
      approvedAt: 1_100,
    };
    expect(
      codeOf(() =>
        settleBatch({
          plan,
          approval,
          attempts: [succeeded("h1", 1_201)],
          startedAt: 1_200,
          finishedAt: 1_202,
        }),
      ),
    ).toBe("planStateInvalid");
  });
});

describe("whole-batch undo", () => {
  async function appliedBatch() {
    return [
      await historyEntry({
        id: "h1",
        planId: "plan-three",
        operationIndex: 0,
        appliedPath: "projects/project-plan.md",
        appliedContent: EDITED_TEXT,
        beforePath: "projects/project-plan.md",
        beforeContent: PLAN_TEXT,
      }),
      await historyEntry({
        id: "h2",
        planId: "plan-three",
        operationIndex: 1,
        appliedPath: "notes/paalala.md",
        appliedContent: "Paalala 23\n",
        beforePath: "notes/paalala.md",
        beforeContent: "Paalala\n",
      }),
    ];
  }

  it("undoes a batch whose files still match what Folio saved", async () => {
    const entries = await appliedBatch();
    const preflight = preflightUndo({
      planId: "plan-three",
      entries,
      observed: await observedPaths({
        "projects/project-plan.md": EDITED_TEXT,
        "notes/paalala.md": "Paalala 23\n",
      }),
    });
    expect(preflight.undoable).toBe(true);
    expect(codeOf(() => assertUndoable(preflight))).toBe("no-error");
  });

  it("changes nothing and names the blocking file when one file was edited", async () => {
    const entries = await appliedBatch();
    const preflight = preflightUndo({
      planId: "plan-three",
      entries,
      observed: await observedPaths({
        "projects/project-plan.md": EDITED_TEXT,
        "notes/paalala.md": "Binago ko ito pagkatapos\n",
      }),
    });
    expect(preflight.undoable).toBe(false);
    expect(preflight.conflicts).toHaveLength(1);
    expect(preflight.conflicts[0].relativePath).toBe("notes/paalala.md");
    expect(preflight.conflicts[0].reason).toBe("externallyModified");
    let details: Record<string, unknown> | undefined;
    try {
      assertUndoable(preflight);
    } catch (cause) {
      if (isFolioError(cause)) details = cause.details;
      expect(isFolioError(cause) && cause.code).toBe("undoConflict");
    }
    expect(details?.blockingRelativePath).toBe("notes/paalala.md");
    // The unmodified entry is still listed, so the user sees the whole batch
    // was refused rather than partially reversed.
    expect(preflight.entryIds).toEqual(["h1", "h2"]);
  });

  it("refuses when a saved file is gone", async () => {
    const entries = await appliedBatch();
    const preflight = preflightUndo({
      planId: "plan-three",
      entries,
      observed: await observedPaths({
        "projects/project-plan.md": EDITED_TEXT,
        "notes/paalala.md": null,
      }),
    });
    expect(preflight.conflicts[0].reason).toBe("missing");
    expect(preflight.conflicts[0].observedContentHash).toBeNull();
  });

  it("refuses to restore a renamed file onto an occupied original name", async () => {
    const entry = await historyEntry({
      id: "h3",
      planId: "plan-rename",
      operationIndex: 0,
      appliedPath: "notes/paalala-oktubre.md",
      appliedContent: "Paalala\n",
      beforePath: "notes/paalala.md",
      beforeContent: "Paalala\n",
    });
    const preflight = preflightUndo({
      planId: "plan-rename",
      entries: [entry],
      observed: {
        "notes/paalala-oktubre.md": {
          exists: true,
          isFile: true,
          contentHash: await hashText("Paalala\n"),
        },
        "notes/paalala.md": {
          exists: true,
          isFile: true,
          contentHash: await hashText("Ibang file na ngayon\n"),
        },
      },
    });
    expect(preflight.undoable).toBe(false);
    expect(preflight.conflicts[0].reason).toBe("destinationOccupied");
    expect(preflight.conflicts[0].relativePath).toBe("notes/paalala.md");
  });

  it("refuses an entry whose previous content was not retained", async () => {
    const entry = await historyEntry({
      id: "h4",
      planId: "plan-three",
      operationIndex: 0,
      appliedPath: "projects/project-plan.md",
      appliedContent: EDITED_TEXT,
      recoverable: false,
    });
    const preflight = preflightUndo({
      planId: "plan-three",
      entries: [entry],
      observed: { "projects/project-plan.md": ABSENT },
    });
    expect(preflight.conflicts[0].reason).toBe("notRecoverable");
  });

  it("ignores entries that were already undone", async () => {
    const [first, second] = await appliedBatch();
    const preflight = preflightUndo({
      planId: "plan-three",
      entries: [{ ...first, undoneAt: 1_600 }, second],
      observed: await observedPaths({ "notes/paalala.md": "Paalala 23\n" }),
    });
    expect(preflight.entryIds).toEqual(["h2"]);
    expect(preflight.undoable).toBe(true);
  });
});

describe("undoing a deletion", () => {
  async function deleted(recoverable = true) {
    return deletionEntry({
      id: "h5",
      planId: "plan-delete",
      path: "notes/paalala.md",
      content: "Paalala\n",
      recoverable,
    });
  }

  it("observes the path the file would be restored to", async () => {
    expect(undoPaths([await deleted()])).toEqual(["notes/paalala.md"]);
  });

  it("re-creates the file while nothing uses its name", async () => {
    const preflight = preflightUndo({
      planId: "plan-delete",
      entries: [await deleted()],
      observed: { "notes/paalala.md": ABSENT },
    });
    expect(preflight.undoable).toBe(true);
    expect(preflight.entryIds).toEqual(["h5"]);
  });

  it("never restores it over a file that now uses the name", async () => {
    const preflight = preflightUndo({
      planId: "plan-delete",
      entries: [await deleted()],
      observed: await observedPaths({ "notes/paalala.md": "Bagong tala\n" }),
    });
    expect(preflight.undoable).toBe(false);
    expect(preflight.conflicts[0].reason).toBe("destinationOccupied");
    expect(preflight.conflicts[0].relativePath).toBe("notes/paalala.md");
    expect(preflight.conflicts[0].observedContentHash).toBe(
      await hashText("Bagong tala\n"),
    );
  });

  it("is not recoverable once its contents are no longer kept", async () => {
    const preflight = preflightUndo({
      planId: "plan-delete",
      entries: [await deleted(false)],
      observed: { "notes/paalala.md": ABSENT },
    });
    expect(preflight.conflicts[0].reason).toBe("notRecoverable");
    expect(preflight.conflicts[0].relativePath).toBe("notes/paalala.md");
  });
});

describe("document identity in operations", () => {
  it("binds an operation to a workspace-scoped identity, not a bare path", async () => {
    const operation = await editOperation(
      "projects/project-plan.md",
      PLAN_TEXT,
      EDITED_TEXT,
    );
    expect(operation.kind).toBe("edit");
    if (operation.kind !== "edit") return;
    expect(operation.documentId).toBe(documentId("projects/project-plan.md"));
    expect(operation.documentId).not.toBe(operation.relativePath);
  });
});

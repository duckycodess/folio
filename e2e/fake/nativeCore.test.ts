import { beforeEach, describe, expect, it } from "vitest";
import type {
  ActionPlan,
  ApplyReport,
  DuplicateGroup,
  ExplicitReference,
  FileOperation,
  HistoryEntry,
  IndexedDocument,
  OrganizationSuggestions,
  ScanSummary,
  SearchResult,
  UndoPreflight,
  UndoReport,
  WorkspaceInfo,
} from "../../src/domain/contracts";
import { hashText } from "../../src/domain/hash";
import { planDigest, preflightPlan } from "../../src/domain/plan";
import { sliceByUtf8Offsets } from "../../src/domain/offsets";
import { fixtureCorpus } from "./corpus";
import { installFakeNativeCore, type FakeScope } from "./nativeCore";
import type { FakeControl, FakeNativeOptions } from "./types";

/**
 * The browser-journey fake is only useful if it answers with the same
 * encodings the shared domain produces. These cases pin it to
 * `src/domain/` — plan digests, preflight refusals, UTF-8 source offsets and
 * content hashes — so a journey that passes against the fake has exercised the
 * frozen contract rather than a convenient imitation of it.
 */
const WORKSPACE_ID = "e2e-workspace";
const PLAN = "projects/project-plan.md";
const COPY = "archive/project-plan-copy.md";

function options(
  overrides: Partial<FakeNativeOptions> = {},
): FakeNativeOptions {
  return {
    workspaceId: WORKSPACE_ID,
    rootPath: "/folio/Community Learning Project",
    authorizedAt: Date.UTC(2026, 9, 10, 8, 0),
    files: fixtureCorpus(),
    preIndexed: true,
    scanStepMs: 0,
    planLifetimeMs: 60_000,
    skipped: [],
    dismissFolderPicker: false,
    ...overrides,
  };
}

const scope = globalThis as unknown as FakeScope;

function call<T>(
  command: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  return scope.__TAURI_INTERNALS__!.invoke(command, args) as Promise<T>;
}

function control(): FakeControl {
  return scope.__folioFake!;
}

async function rejection(
  command: string,
  args: Record<string, unknown> = {},
): Promise<{ code: string; details?: Record<string, string> }> {
  try {
    await call(command, args);
  } catch (cause) {
    return cause as { code: string; details?: Record<string, string> };
  }
  throw new Error(`${command} resolved where a failure was expected`);
}

/** One rename of the project plan, pinned to the revision the index holds. */
async function renameOperation(): Promise<FileOperation> {
  const indexed = await call<IndexedDocument[]>("list_indexed_documents", {
    workspaceId: WORKSPACE_ID,
  });
  const document = indexed.find((item) => item.relativePath === PLAN)!;
  return {
    kind: "rename",
    documentId: document.id,
    relativePath: PLAN,
    expectedContentHash: document.contentHash,
    destinationRelativePath: "projects/community-learning-project.md",
    expectedDestination: "absent",
  };
}

describe("the browser-journey fake native core", () => {
  beforeEach(async () => {
    installFakeNativeCore(options());
    await call<WorkspaceInfo | null>("choose_workspace");
  });

  it("identifies documents and hashes exactly as the shared domain does", async () => {
    const corpus = fixtureCorpus();
    const indexed = await call<IndexedDocument[]>("list_indexed_documents", {
      workspaceId: WORKSPACE_ID,
    });
    const plan = indexed.find((item) => item.relativePath === PLAN)!;
    const source = corpus.find((file) => file.relativePath === PLAN)!;
    expect(plan.id).toBe(`${WORKSPACE_ID}:${PLAN}`);
    expect(plan.contentHash).toBe(await hashText(source.content));
    expect(plan.sizeBytes).toBe(
      new TextEncoder().encode(source.content).length,
    );
    // The index does not guess a language; detection belongs to the provider.
    expect(plan.language).toBe("unknown");
  });

  it("locates search evidence at real UTF-8 byte offsets in Filipino text", async () => {
    const results = await call<SearchResult[]>("search_index", {
      workspaceId: WORKSPACE_ID,
      query: "huling araw ng pagpasa",
      limit: 10,
    });
    const hit = results.find(
      (result) => result.document.relativePath === "notes/tala-sa-proyekto.md",
    )!;
    expect(hit.method).toBe("keyword");
    const content = fixtureCorpus().find(
      (file) => file.relativePath === "notes/tala-sa-proyekto.md",
    )!.content;
    const passage = hit.passages[0];
    expect(passage.offsetUnit).toBe("utf8Byte");
    expect(sliceByUtf8Offsets(content, passage.start, passage.end)).toBe(
      passage.text,
    );
    expect(passage.text).toContain("huling araw ng pagpasa");
    expect(passage.documentContentHash).toBe(await hashText(content));
  });

  it("reads a text PDF's pages and keeps the file's own size and hash", async () => {
    const results = await call<SearchResult[]>("search_index", {
      workspaceId: WORKSPACE_ID,
      query: "consent",
      limit: 10,
    });
    const hit = results.find((result) =>
      result.document.relativePath.endsWith(".pdf"),
    )!;
    const source = fixtureCorpus().find((file) =>
      file.relativePath.endsWith(".pdf"),
    )!;
    expect(hit.document.sizeBytes).toBe(source.fileSizeBytes);
    expect(hit.document.contentHash).toBe(source.fileContentHash);
    expect(hit.passages.some((passage) => passage.page === 1)).toBe(true);
  });

  it("issues plan digests the shared canonical encoding reproduces", async () => {
    const plan = await call<ActionPlan>("prepare_plan", {
      workspaceId: WORKSPACE_ID,
      operations: [await renameOperation()],
    });
    expect(plan.digest).toBe(await planDigest(plan));
    expect(plan.impacts).toEqual([]);
  });

  it("refuses the same plans the shared preflight refuses", async () => {
    const rename = await renameOperation();
    const cases: { operations: FileOperation[]; expected: string }[] = [
      { operations: [], expected: "planEmpty" },
      { operations: [rename, rename], expected: "duplicateOperationTarget" },
      {
        operations: [
          {
            ...rename,
            destinationRelativePath: COPY,
          } as FileOperation,
        ],
        expected: "destinationExists",
      },
      {
        operations: [
          {
            ...rename,
            expectedContentHash: `sha256:${"0".repeat(64)}`,
          } as FileOperation,
        ],
        expected: "targetChanged",
      },
      {
        operations: [
          { ...rename, relativePath: "projects/gone.md" } as FileOperation,
        ],
        expected: "targetMissing",
      },
      {
        operations: [
          {
            ...rename,
            destinationRelativePath: "../escape.md",
          } as FileOperation,
        ],
        expected: "pathEscapesWorkspace",
      },
    ];
    for (const { operations, expected } of cases) {
      const refusal = await rejection("prepare_plan", {
        workspaceId: WORKSPACE_ID,
        operations,
      });
      expect(refusal.code, JSON.stringify(operations)).toBe(expected);
      // The same operations, judged by the shared implementation, are refused
      // the same way: the plan's own identity and window are valid.
      if (operations.length) {
        const now = Date.now();
        const candidate: ActionPlan = {
          id: "check",
          workspaceId: WORKSPACE_ID,
          createdAt: now,
          expiresAt: now + 60_000,
          operations,
          impacts: [],
          digest: `sha256:${"0".repeat(64)}`,
        };
        const observed: Record<
          string,
          { exists: boolean; contentHash?: string }
        > = {};
        for (const file of fixtureCorpus())
          observed[file.relativePath] = {
            exists: true,
            contentHash: file.fileContentHash ?? (await hashText(file.content)),
            isFile: true,
          } as never;
        observed["projects/gone.md"] = { exists: false };
        observed["projects/community-learning-project.md"] = { exists: false };
        observed["../escape.md"] = { exists: false };
        let shared = "";
        try {
          preflightPlan(candidate, observed as never, now);
        } catch (cause) {
          shared = (cause as { code: string }).code;
        }
        expect(shared, JSON.stringify(operations)).toBe(expected);
      }
    }
  });

  it("writes nothing until an approval echoes the digest it was shown", async () => {
    const plan = await call<ActionPlan>("prepare_plan", {
      workspaceId: WORKSPACE_ID,
      operations: [await renameOperation()],
    });
    expect(control().readFile(PLAN)).not.toBeNull();

    expect(
      (
        await rejection("apply_plan", {
          workspaceId: WORKSPACE_ID,
          planId: plan.id,
        })
      ).code,
    ).toBe("approvalRequired");
    expect(
      (
        await rejection("approve_plan", {
          workspaceId: WORKSPACE_ID,
          planId: plan.id,
          planDigest: `sha256:${"0".repeat(64)}`,
        })
      ).code,
    ).toBe("planDigestMismatch");
    expect(control().readFile(PLAN)).not.toBeNull();

    await call("approve_plan", {
      workspaceId: WORKSPACE_ID,
      planId: plan.id,
      planDigest: plan.digest,
    });
    const report = await call<ApplyReport>("apply_plan", {
      workspaceId: WORKSPACE_ID,
      planId: plan.id,
    });
    expect(report.batch.stopReason).toBe("completed");
    expect(report.batch.outcomes[0].status).toBe("succeeded");
    expect(control().readFile(PLAN)).toBeNull();
    expect(
      control().readFile("projects/community-learning-project.md"),
    ).toContain("Community Learning Project");
    // The approval is spent: the same plan cannot be applied twice.
    expect(
      (
        await rejection("apply_plan", {
          workspaceId: WORKSPACE_ID,
          planId: plan.id,
        })
      ).code,
    ).toBe("planStateInvalid");
  });

  it("keeps earlier changes when an operation fails partway through a batch", async () => {
    const suggestions = await call<OrganizationSuggestions>(
      "organization_suggestions",
      { workspaceId: WORKSPACE_ID },
    );
    const operations = suggestions.filenames
      .slice(0, 3)
      .map((item) => item.operation);
    control().setWriter({ failAtIndex: 1, failCode: "destinationExists" });
    const plan = await call<ActionPlan>("prepare_plan", {
      workspaceId: WORKSPACE_ID,
      operations,
    });
    await call("approve_plan", {
      workspaceId: WORKSPACE_ID,
      planId: plan.id,
      planDigest: plan.digest,
    });
    const report = await call<ApplyReport>("apply_plan", {
      workspaceId: WORKSPACE_ID,
      planId: plan.id,
    });
    expect(report.batch.stopReason).toBe("failed");
    expect(report.batch.outcomes.map((outcome) => outcome.status)).toEqual([
      "succeeded",
      "failed",
      "notStarted",
    ]);
    expect(report.batch.outcomes[1].error?.code).toBe("destinationExists");
    // The first file really moved; the second was left exactly as it was.
    const renames = operations as Extract<
      FileOperation,
      { kind: "rename" | "move" }
    >[];
    expect(
      control().readFile(renames[0].destinationRelativePath),
    ).not.toBeNull();
    expect(control().readFile(renames[1].relativePath)).not.toBeNull();
    expect(control().readFile(renames[1].destinationRelativePath)).toBeNull();
    const history = await call<HistoryEntry[]>("list_history", {
      workspaceId: WORKSPACE_ID,
    });
    expect(history.filter((entry) => entry.planId === plan.id)).toHaveLength(1);
  });

  it("reports historyRequired for a change it made but cannot reverse", async () => {
    control().setWriter({ historyRequiredAtIndex: 0 });
    const operation = await renameOperation();
    const plan = await call<ActionPlan>("prepare_plan", {
      workspaceId: WORKSPACE_ID,
      operations: [operation],
    });
    await call("approve_plan", {
      workspaceId: WORKSPACE_ID,
      planId: plan.id,
      planDigest: plan.digest,
    });
    const report = await call<ApplyReport>("apply_plan", {
      workspaceId: WORKSPACE_ID,
      planId: plan.id,
    });
    expect(report.batch.outcomes[0].error?.code).toBe("historyRequired");
    // The file did change; only the record of how to reverse it is missing.
    expect(control().readFile(PLAN)).toBeNull();
    expect(
      control().readFile("projects/community-learning-project.md"),
    ).not.toBeNull();
    // Nothing was recorded, so Undo has nothing to reverse and says so.
    const preview = await call<UndoPreflight>("preview_undo", {
      workspaceId: WORKSPACE_ID,
      planId: plan.id,
    });
    expect(preview.entryIds).toEqual([]);
    expect(
      (
        await rejection("undo_plan", {
          workspaceId: WORKSPACE_ID,
          planId: plan.id,
          entryIds: [],
        })
      ).code,
    ).toBe("planStateInvalid");
  });

  it("refuses a whole-batch Undo when a file changed afterwards", async () => {
    const operation = await renameOperation();
    const plan = await call<ActionPlan>("prepare_plan", {
      workspaceId: WORKSPACE_ID,
      operations: [operation],
    });
    await call("approve_plan", {
      workspaceId: WORKSPACE_ID,
      planId: plan.id,
      planDigest: plan.digest,
    });
    await call<ApplyReport>("apply_plan", {
      workspaceId: WORKSPACE_ID,
      planId: plan.id,
    });
    control().externalEdit(
      "projects/community-learning-project.md",
      "# Changed by another app\n",
    );
    const preview = await call<UndoPreflight>("preview_undo", {
      workspaceId: WORKSPACE_ID,
      planId: plan.id,
    });
    expect(preview.undoable).toBe(false);
    expect(preview.conflicts[0].reason).toBe("externallyModified");
    const refusal = await rejection("undo_plan", {
      workspaceId: WORKSPACE_ID,
      planId: plan.id,
      entryIds: preview.entryIds,
    });
    expect(refusal.code).toBe("undoConflict");
    expect(refusal.details?.blockingRelativePath).toBe(
      "projects/community-learning-project.md",
    );
    // The newer external edit survived, and the old name was not restored.
    expect(control().readFile("projects/community-learning-project.md")).toBe(
      "# Changed by another app\n",
    );
    expect(control().readFile(PLAN)).toBeNull();
  });

  it("undoes a clean batch and puts the file back where it was", async () => {
    const before = control().readFile(PLAN);
    const plan = await call<ActionPlan>("prepare_plan", {
      workspaceId: WORKSPACE_ID,
      operations: [await renameOperation()],
    });
    await call("approve_plan", {
      workspaceId: WORKSPACE_ID,
      planId: plan.id,
      planDigest: plan.digest,
    });
    await call<ApplyReport>("apply_plan", {
      workspaceId: WORKSPACE_ID,
      planId: plan.id,
    });
    const preview = await call<UndoPreflight>("preview_undo", {
      workspaceId: WORKSPACE_ID,
      planId: plan.id,
    });
    expect(preview.undoable).toBe(true);
    const report = await call<UndoReport>("undo_plan", {
      workspaceId: WORKSPACE_ID,
      planId: plan.id,
      entryIds: preview.entryIds,
    });
    expect(report.undoneEntryIds).toEqual(preview.entryIds);
    expect(report.remainingEntryIds).toEqual([]);
    expect(control().readFile(PLAN)).toBe(before);
  });

  it("expires a preview rather than applying a stale one", async () => {
    const plan = await call<ActionPlan>("prepare_plan", {
      workspaceId: WORKSPACE_ID,
      operations: [await renameOperation()],
    });
    control().expirePlans();
    expect(
      (
        await rejection("approve_plan", {
          workspaceId: WORKSPACE_ID,
          planId: plan.id,
          planDigest: plan.digest,
        })
      ).code,
    ).toBe("planExpired");
    expect(control().readFile(PLAN)).not.toBeNull();
  });

  it("finds exact duplicates by content and explicit links by evidence", async () => {
    const duplicates = await call<DuplicateGroup[]>("list_duplicates", {
      workspaceId: WORKSPACE_ID,
    });
    expect(duplicates).toHaveLength(1);
    expect(duplicates[0].documents.map((item) => item.relativePath)).toEqual([
      COPY,
      PLAN,
    ]);
    const references = await call<ExplicitReference[]>("list_relationships", {
      workspaceId: WORKSPACE_ID,
    });
    const fromNotes = references.find(
      (reference) =>
        reference.sourceId === `${WORKSPACE_ID}:meetings/meeting-notes.md`,
    )!;
    expect(fromNotes.link.resolvedRelativePath).toBe(PLAN);
    expect(fromNotes.evidence[0].text).toContain("[project plan]");
    // The duplicate carries the links of the folder it was copied from. The one
    // that still resolves is a reference; the one that no longer does is not
    // invented as a valid one.
    const fromCopy = references.filter(
      (reference) => reference.sourceId === `${WORKSPACE_ID}:${COPY}`,
    );
    expect(
      fromCopy.map((reference) => reference.link.resolvedRelativePath),
    ).toEqual(["meetings/meeting-notes.md"]);
  });

  it("stops a scan with a cancelled summary rather than an error", async () => {
    installFakeNativeCore(options({ preIndexed: false, scanStepMs: 5 }));
    await call<WorkspaceInfo | null>("choose_workspace");
    const scan = call<ScanSummary>("scan_workspace", {
      workspaceId: WORKSPACE_ID,
      recheckUnreadable: false,
    });
    await new Promise((resolve) => setTimeout(resolve, 12));
    await call("cancel_indexing");
    const summary = await scan;
    expect(summary.cancelled).toBe(true);
    // Whatever was already indexed is kept, and the rest is simply not there.
    expect(summary.total).toBeGreaterThan(0);
    expect(summary.total).toBeLessThan(fixtureCorpus().length);
  });

  it("rejects with the plain wire failure shape, for any injected code", async () => {
    control().failNext("read_document", {
      code: "documentTooLarge",
      message: "Too large.",
      details: { path: PLAN },
    });
    const refusal = await rejection("read_document", {
      workspaceId: WORKSPACE_ID,
      relativePath: PLAN,
    });
    expect(refusal).toEqual({
      code: "documentTooLarge",
      message: "Too large.",
      details: { path: PLAN },
    });
    expect(refusal).not.toBeInstanceOf(Error);
  });
});

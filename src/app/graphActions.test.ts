import { describe, expect, it, vi } from "vitest";
import type {
  ActionPlan,
  ApplyReport,
  DocumentRecord,
} from "../domain/contracts";
import { folioError } from "../domain/errors";
import {
  deleteOperation,
  deleteStep,
  graphActions,
  prepareDelete,
} from "./graphActions";
import {
  ORGANIZE_START,
  organizeFlow,
  type OrganizeEvent,
  type OrganizeState,
} from "./organizeFlow";

const desktopFolder = { source: "folder" as const, nativeAvailable: true };
const markdown = { mediaType: "text/markdown" as const };

describe("Graph node actions", () => {
  it("offers rename, move, edit and delete for a text file in a folder", () => {
    const actions = graphActions(desktopFolder, markdown);
    expect(actions.map((action) => action.kind)).toEqual([
      "rename",
      "move",
      "edit",
      "delete",
    ]);
    expect(actions.every((action) => !action.disabledReason)).toBe(true);
  });

  it("lists every action for a PDF but lets it only be opened", () => {
    const actions = graphActions(desktopFolder, {
      mediaType: "application/pdf",
    });
    expect(actions).toHaveLength(4);
    for (const action of actions)
      expect(action.disabledReason).toMatch(/PDFs can only be opened/);
  });

  it("never offers a change for sample files or in the browser preview", () => {
    for (const action of graphActions(
      { source: "samples", nativeAvailable: true },
      markdown,
    ))
      expect(action.disabledReason).toMatch(/Sample files can't be changed/);
    for (const action of graphActions(
      { source: "samples", nativeAvailable: false },
      markdown,
    ))
      expect(action.disabledReason).toMatch(/desktop app/);
  });
});

describe("deleting from Graph", () => {
  it("pins the revision Folio read, with no destination", () => {
    expect(
      deleteOperation({
        id: "w:notes/paalala.md",
        relativePath: "notes/paalala.md",
        contentHash: "sha256:abc",
      }),
    ).toEqual({
      kind: "delete",
      documentId: "w:notes/paalala.md",
      relativePath: "notes/paalala.md",
      expectedContentHash: "sha256:abc",
    });
  });
});

describe("preparing a deletion", () => {
  const listed = {
    id: "w:notes/paalala.md",
    relativePath: "notes/paalala.md",
    contentHash: "sha256:listed",
  } as DocumentRecord;
  const reread = { ...listed, contentHash: "sha256:changed" };

  it("pins the listed revision without another read", async () => {
    const read = vi.fn(async () => reread);
    const [operation] = await prepareDelete(listed, false, read);
    expect(read).not.toHaveBeenCalled();
    expect(operation).toMatchObject({ expectedContentHash: "sha256:listed" });
  });

  it("reads the file again for Preview again, so a stale hash isn't resent", async () => {
    const read = vi.fn(async () => reread);
    const [operation] = await prepareDelete(listed, true, read);
    expect(read).toHaveBeenCalledOnce();
    expect(operation).toMatchObject({ expectedContentHash: "sha256:changed" });
  });

  it("refuses to build a plan without a hash", async () => {
    const unread = { ...listed, contentHash: undefined };
    await expect(
      prepareDelete(unread, false, async () => unread),
    ).rejects.toMatchObject({ code: "internal" });
  });
});

describe("the Delete dialog's steps", () => {
  const PLAN = {
    id: "plan-1",
    operations: [],
    impacts: [],
  } as unknown as ActionPlan;
  const OPENED = { available: true, started: true };
  const STALE = folioError(
    "targetChanged",
    "The file changed since the preview.",
  );

  function run(events: OrganizeEvent[], from: OrganizeState = ORGANIZE_START) {
    return events.reduce(organizeFlow, from);
  }

  const previewed = run([
    { type: "prepareStarted", request: 1, operations: [] },
    { type: "prepared", request: 1, plan: PLAN },
  ]);

  it("shows progress until the preview arrives, then the preview", () => {
    expect(
      deleteStep(ORGANIZE_START, { available: true, started: false }),
    ).toBe("preparing");
    expect(deleteStep(previewed, OPENED)).toBe("preview");
  });

  it("stays open through a stale refusal and its Preview again", () => {
    // Approve, then the native core refuses: the preview stays, with the error.
    const refused = run(
      [
        { type: "applyStarted", request: 2 },
        { type: "failed", request: 2, error: STALE },
      ],
      previewed,
    );
    expect(refused.stage).toBe("preview");
    expect(refused.error).toBe(STALE);
    expect(deleteStep(refused, OPENED)).toBe("preview");

    // Preview again is a new prepare; the dialog must not close for it.
    const again = run(
      [{ type: "prepareStarted", request: 3, operations: [] }],
      refused,
    );
    expect(deleteStep(again, OPENED)).toBe("preparing");
    expect(
      deleteStep(
        run([{ type: "prepared", request: 3, plan: PLAN }], again),
        OPENED,
      ),
    ).toBe("preview");
  });

  it("offers Retry when the preview can't be built", () => {
    const failed = run([
      { type: "prepareStarted", request: 1, operations: [] },
      { type: "failed", request: 1, error: STALE },
    ]);
    expect(deleteStep(failed, OPENED)).toBe("error");
  });

  it("closes only when the flow is reset without a result", () => {
    expect(
      deleteStep(run([{ type: "reset", request: 9 }], previewed), OPENED),
    ).toBe("closed");
    const applied = run(
      [
        { type: "applyStarted", request: 2 },
        {
          type: "applied",
          request: 2,
          report: { batch: { outcomes: [] } } as unknown as ApplyReport,
        },
      ],
      previewed,
    );
    expect(deleteStep(applied, OPENED)).toBe("result");
  });

  it("never prepares anything for a file that can't be changed", () => {
    expect(deleteStep(previewed, { available: false, started: false })).toBe(
      "unavailable",
    );
  });
});

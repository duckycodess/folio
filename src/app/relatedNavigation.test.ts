import { describe, expect, it } from "vitest";
import type { DocumentRecord, SourcePassage } from "../domain/contracts";
import { documentId } from "../domain/test-support";
import {
  NO_NAVIGATION,
  relatedNavigation,
  type RelatedNavigationAction,
} from "./relatedNavigation";

const PLAN = { id: documentId("projects/project-plan.md") } as DocumentRecord;
const NOTES = { id: documentId("meetings/meeting-notes.md") } as DocumentRecord;
const BUDGET = { id: documentId("personal/budget.md") } as DocumentRecord;
const PASSAGE_IN_NOTES = {
  documentId: NOTES.id,
  start: 0,
  end: 5,
} as SourcePassage;

function run(...actions: RelatedNavigationAction[]) {
  return actions.reduce(relatedNavigation, NO_NAVIGATION);
}

describe("navigating through related files", () => {
  it("keeps the way back while Folio follows evidence", () => {
    const state = run(
      { type: "openPassage", passage: PASSAGE_IN_NOTES, current: PLAN },
      { type: "selectionChanged", id: NOTES.id },
    );
    expect(state.trail).toEqual([PLAN]);
    expect(state.focus).toBe(PASSAGE_IN_NOTES);
  });

  it("forgets an old highlight once a file is picked elsewhere", () => {
    const state = run(
      { type: "openPassage", passage: PASSAGE_IN_NOTES, current: PLAN },
      { type: "selectionChanged", id: NOTES.id },
      // The user picks other files from the list, then the same one again.
      { type: "selectionChanged", id: BUDGET.id },
      { type: "selectionChanged", id: NOTES.id },
    );
    expect(state).toEqual(NO_NAVIGATION);
  });

  it("returns to the origin's Related tab on Back", () => {
    const state = run(
      { type: "openRelated", from: PLAN, to: NOTES },
      { type: "selectionChanged", id: NOTES.id },
      { type: "back" },
      { type: "selectionChanged", id: PLAN.id },
    );
    expect(state.trail).toEqual([]);
    expect(state.returnedTo).toBe(PLAN.id);
  });

  it("highlights a passage in the open file without adding to the trail", () => {
    const state = run({
      type: "openPassage",
      passage: PASSAGE_IN_NOTES,
      current: NOTES,
    });
    expect(state.trail).toEqual([]);
    expect(state.focus).toBe(PASSAGE_IN_NOTES);
  });
});

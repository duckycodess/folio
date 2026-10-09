import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { AiIndexProvider, type AiIndexState } from "../app/useAiIndex";
import { RIPPLE_INCOMPLETE_WARNING } from "../domain/aiCoverage";
import type { AiRelationshipCoverage } from "../domain/contracts";
import { ImpactList } from "./PlanReview";

function state(coverage: AiRelationshipCoverage | null): AiIndexState {
  return {
    coverage,
    refreshing: false,
    phase: null,
    pairsCompleted: 0,
    failure: null,
    refresh: () => {},
    stop: () => {},
  };
}

const partial: AiRelationshipCoverage = {
  state: "partial",
  eligibleDocuments: 3,
  indexedDocuments: 3,
  pairsConsidered: 1,
  pairsRemaining: 2,
  overflowDocuments: 0,
};

const decode = (html: string) => html.replaceAll("&#x27;", "'");

function render(coverage: AiRelationshipCoverage | null) {
  return decode(
    renderToStaticMarkup(
      createElement(
        AiIndexProvider,
        { value: state(coverage) },
        createElement(ImpactList, { impacts: [], generationReady: false }),
      ),
    ),
  );
}

describe("Ripple's incomplete-AI-review warning", () => {
  it("shows only while AI review is unfinished, in the same list that approval sits beside", () => {
    expect(render(partial)).toContain(RIPPLE_INCOMPLETE_WARNING);
    expect(render({ ...partial, state: "noActiveSpace" })).toContain(
      RIPPLE_INCOMPLETE_WARNING,
    );
    expect(render({ ...partial, state: "complete" })).not.toContain(
      RIPPLE_INCOMPLETE_WARNING,
    );
    // The browser preview has no coverage, so no warning either way.
    expect(render(null)).not.toContain(RIPPLE_INCOMPLETE_WARNING);
  });

  it("is a note, not a blocking control", () => {
    const html = render(partial);
    expect(html).toContain('role="note"');
    expect(html).not.toMatch(/<button[^>]*disabled/);
  });
});

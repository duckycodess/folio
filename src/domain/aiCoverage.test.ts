import { describe, expect, it } from "vitest";
import {
  coverageNotice,
  mayClaimNoConnections,
  RIPPLE_INCOMPLETE_WARNING,
  rippleWarning,
  shouldAutoRefresh,
  summaryBasisLine,
} from "./aiCoverage";
import type { AiRelationshipCoverage } from "./contracts";

const base: AiRelationshipCoverage = {
  state: "complete",
  eligibleDocuments: 4,
  indexedDocuments: 4,
  pairsConsidered: 6,
  pairsRemaining: 0,
  overflowDocuments: 0,
};

describe("AI coverage wording", () => {
  it("never lets an incomplete folder claim there are no connections", () => {
    for (const state of [
      "noActiveSpace",
      "embeddingIncomplete",
      "partial",
    ] as const) {
      expect(mayClaimNoConnections({ ...base, state })).toBe(false);
      expect(coverageNotice({ ...base, state })?.message).not.toMatch(
        /no (ai )?connections/i,
      );
    }
    expect(mayClaimNoConnections(base)).toBe(true);
    expect(mayClaimNoConnections(null)).toBe(false);
  });

  it("explains each unfinished state and offers to continue only when it can", () => {
    expect(coverageNotice({ ...base, state: "noActiveSpace" })).toEqual({
      message: "AI connections need the search model and its index.",
      canContinue: false,
    });
    const preparing = coverageNotice({
      ...base,
      state: "embeddingIncomplete",
      eligibleDocuments: 1,
    });
    expect(preparing?.message).toContain("1 of 4 files");
    expect(preparing?.canContinue).toBe(true);
    expect(
      coverageNotice({ ...base, state: "partial" }, true)?.canContinue,
    ).toBe(false);
  });

  it("reports truncation only once the comparison is complete", () => {
    expect(coverageNotice(base)).toBeNull();
    expect(coverageNotice({ ...base, overflowDocuments: 1 })?.message).toBe(
      "Some AI connections for 1 file were truncated.",
    );
    expect(coverageNotice(null)).toBeNull();
  });

  it("warns in Ripple without claiming anything about links or copies", () => {
    expect(rippleWarning({ ...base, state: "partial" })).toBe(
      RIPPLE_INCOMPLETE_WARNING,
    );
    expect(rippleWarning({ ...base, state: "noActiveSpace" })).toBe(
      RIPPLE_INCOMPLETE_WARNING,
    );
    expect(rippleWarning(base)).toBeNull();
    expect(rippleWarning(null)).toBeNull();
    expect(RIPPLE_INCOMPLETE_WARNING).not.toMatch(/link|cop(y|ies)/i);
  });

  it("refreshes automatically only with a ready model, an open folder and no refresh running", () => {
    const ready = {
      searchModelReady: true,
      refreshing: false,
      folderOpen: true,
    };
    expect(shouldAutoRefresh(ready)).toBe(true);
    expect(shouldAutoRefresh({ ...ready, searchModelReady: false })).toBe(
      false,
    );
    expect(shouldAutoRefresh({ ...ready, refreshing: true })).toBe(false);
    expect(shouldAutoRefresh({ ...ready, folderOpen: false })).toBe(false);
  });

  it("states the connections and files a summary was given, and when that is incomplete", () => {
    expect(
      summaryBasisLine({ connections: 3, files: 4, incomplete: false }),
    ).toBe("Based on 3 connections across 4 files.");
    expect(
      summaryBasisLine({ connections: 1, files: 2, incomplete: true }),
    ).toBe(
      "Based on 1 connection across 2 files. Incomplete: AI review wasn't finished or some connections were left out.",
    );
  });
});

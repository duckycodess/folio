import type { AiRelationshipCoverage } from "./contracts";

/**
 * Shown in Plan Review when AI review isn't finished. It says nothing about
 * links or identical copies, which Ripple always checks, and it never blocks
 * approval.
 */
export const RIPPLE_INCOMPLETE_WARNING =
  "AI review isn't finished for this folder, so Ripple may not list every AI-related file.";

export interface CoverageNoticeText {
  /** What to tell the person. Never claims "no connections" while incomplete. */
  message: string;
  /** Whether more checking can be started from here. */
  canContinue: boolean;
}

function files(count: number): string {
  return `${count} file${count === 1 ? "" : "s"}`;
}

/**
 * The line that qualifies a short or empty connection list. `null` once every
 * pair of embedded files was compared and nothing was truncated, or when
 * coverage hasn't been read (the browser preview has none).
 */
export function coverageNotice(
  coverage: AiRelationshipCoverage | null,
  refreshing = false,
): CoverageNoticeText | null {
  if (!coverage) return null;
  switch (coverage.state) {
    case "noActiveSpace":
      return {
        message: "AI connections need the search model and its index.",
        canContinue: false,
      };
    case "embeddingIncomplete":
      return {
        message: `Still preparing the search index (${coverage.eligibleDocuments} of ${files(coverage.indexedDocuments)} ready). AI connections will follow.`,
        canContinue: !refreshing,
      };
    case "partial":
      return {
        message: `Still checking AI connections for ${files(coverage.eligibleDocuments)}. Some may not be listed yet.`,
        canContinue: !refreshing,
      };
    case "complete":
      return coverage.overflowDocuments > 0
        ? {
            message: `Some AI connections for ${files(coverage.overflowDocuments)} were truncated.`,
            canContinue: false,
          }
        : null;
  }
}

/** Only a finished comparison may say a file has no AI connections. */
export function mayClaimNoConnections(
  coverage: AiRelationshipCoverage | null,
): boolean {
  return coverage?.state === "complete" && coverage.overflowDocuments === 0;
}

/**
 * The Ripple warning, or `null` when AI review is finished or unavailable
 * (no coverage read, as in the browser preview).
 */
export function rippleWarning(
  coverage: AiRelationshipCoverage | null,
): string | null {
  if (!coverage || coverage.state === "complete") return null;
  return RIPPLE_INCOMPLETE_WARNING;
}

/**
 * Whether a finished Local Sync, or an applied change or Undo, should start an
 * AI refresh: only with a ready search model and nothing already running.
 */
export function shouldAutoRefresh(input: {
  searchModelReady: boolean;
  refreshing: boolean;
  folderOpen: boolean;
}): boolean {
  return input.searchModelReady && input.folderOpen && !input.refreshing;
}

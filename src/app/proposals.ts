import type { FileOperation, OperationProposal } from "../domain/contracts";

/**
 * The plan operation for a proposal that needs no reading: rename, move or
 * create. An edit is built by the native core from the exact passage
 * (`prepare_passage_edit`), so it returns `null` here.
 */
export function proposalOperation(
  proposal: OperationProposal,
): FileOperation | null {
  switch (proposal.kind) {
    case "rename":
    case "move":
      return {
        kind: proposal.kind,
        documentId: proposal.documentId,
        relativePath: proposal.relativePath,
        expectedContentHash: proposal.observedContentHash,
        destinationRelativePath: proposal.destinationRelativePath,
        expectedDestination: "absent",
      };
    case "create":
      return {
        kind: "create",
        destinationRelativePath: proposal.destinationRelativePath,
        mediaType: proposal.destinationRelativePath
          .toLocaleLowerCase()
          .endsWith(".txt")
          ? "text/plain"
          : "text/markdown",
        content: proposal.content,
        expectedDestination: "absent",
      };
    case "edit":
      return null;
  }
}

export interface ChangedRegion {
  /** 1-based line where the change starts. */
  line: number;
  /** Unchanged text just before and after the change, for context. */
  before: string;
  after: string;
  removed: string;
  added: string;
  /** Whether context was cut off at either end. */
  clippedStart: boolean;
  clippedEnd: boolean;
}

/**
 * The one region where `previous` and `next` differ, with some unchanged text
 * around it. Exact: removed and added are the full differing text.
 */
export function changedRegion(
  previous: string,
  next: string,
  context = 80,
): ChangedRegion | null {
  if (previous === next) return null;
  let start = 0;
  const limit = Math.min(previous.length, next.length);
  while (start < limit && previous[start] === next[start]) start++;
  let end = 0;
  while (
    end < limit - start &&
    previous[previous.length - 1 - end] === next[next.length - 1 - end]
  )
    end++;
  const contextStart = Math.max(0, start - context);
  const suffixStart = previous.length - end;
  const contextEnd = Math.min(previous.length, suffixStart + context);
  return {
    line: previous.slice(0, start).split("\n").length,
    before: previous.slice(contextStart, start),
    after: previous.slice(suffixStart, contextEnd),
    removed: previous.slice(start, suffixStart),
    added: next.slice(start, next.length - end),
    clippedStart: contextStart > 0,
    clippedEnd: contextEnd < previous.length,
  };
}

/** The request to send again once the user has named the file. */
export function requestForFile(request: string, relativePath: string): string {
  return `${request.trim()}\n\nUse this file: ${relativePath}`;
}

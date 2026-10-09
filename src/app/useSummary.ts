import { useSyncExternalStore } from "react";
import { cancelGeneration, summarizeDocument } from "../adapters/ai";
import type { DocumentId, WorkspaceId } from "../domain/contracts";
import { toFolioError } from "../domain/errors";
import { createSummaryStore, type SummaryEntry } from "./summaries";

/**
 * Every file's summary lives here, whichever screen asked for it, so the
 * file's Summary tab always shows it. A summary keeps running when its panel
 * closes and is there when the file opens again.
 */
export const summaries = createSummaryStore();

/** Starts a summary for one file. Only one generative request runs at a time. */
export async function summarize(
  workspaceId: WorkspaceId,
  documentId: DocumentId,
): Promise<void> {
  if (summaries.running()) return;
  summaries.set(documentId, { status: "running", startedAt: Date.now() });
  try {
    const result = await summarizeDocument(workspaceId, documentId);
    summaries.set(documentId, { status: "done", result, madeAt: Date.now() });
  } catch (cause) {
    const error = toFolioError(cause);
    summaries.set(
      documentId,
      error.code === "cancelled"
        ? { status: "cancelled" }
        : { status: "failed", error },
    );
  }
}

export function cancelSummary(): Promise<void> {
  return cancelGeneration().catch(() => undefined);
}

export function useSummary(documentId: DocumentId): {
  entry: SummaryEntry | undefined;
  /** Another file's summary is running, so this one has to wait. */
  busyElsewhere: boolean;
  clear: () => void;
} {
  useSyncExternalStore(summaries.subscribe, summaries.version);
  const running = summaries.running();
  return {
    entry: summaries.get(documentId),
    busyElsewhere: running !== null && running !== documentId,
    clear: () => summaries.set(documentId, undefined),
  };
}

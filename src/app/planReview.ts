import type {
  ActionPlan,
  ApplyReport,
  FileOperation,
  OperationStatus,
  UndoPreflight,
  UndoReport,
} from "../domain/contracts";
import { recoveryFor } from "./recovery";

/** One line of an exact preview: what happens to which path. */
export interface PlanRow {
  action: "Rename" | "Move" | "Edit" | "Create";
  /** Absent for a new file. */
  from?: string;
  to: string;
}

export function planRow(operation: FileOperation): PlanRow {
  switch (operation.kind) {
    case "rename":
    case "move":
      return {
        action: operation.kind === "rename" ? "Rename" : "Move",
        from: operation.relativePath,
        to: operation.destinationRelativePath,
      };
    case "edit":
      return {
        action: "Edit",
        from: operation.relativePath,
        to: operation.relativePath,
      };
    case "create":
      return { action: "Create", to: operation.destinationRelativePath };
  }
}

export interface OutcomeRow extends PlanRow {
  status: OperationStatus;
  /** Plain-language reason for a failed operation. */
  reason?: string;
}

export interface ApplySummary {
  /** `saved` only when every change succeeded and the index caught up. */
  tone: "saved" | "partial" | "nothingSaved";
  headline: string;
  details: string[];
  rows: OutcomeRow[];
  /** Whether any change was saved, so Undo is worth offering. */
  undoable: boolean;
}

function changes(count: number): string {
  return `${count} ${count === 1 ? "change" : "changes"}`;
}

/**
 * What actually happened to an approved plan, from the native report alone.
 * Each claim follows from the per-operation outcomes, so a batch that stopped
 * partway never says that nothing changed.
 */
export function summarizeApply(
  plan: ActionPlan,
  report: ApplyReport,
): ApplySummary {
  const rows: OutcomeRow[] = plan.operations.map((operation, index) => {
    const outcome = report.batch.outcomes.find(
      (item) => item.operationIndex === index,
    );
    const status = outcome?.status ?? "notStarted";
    return {
      ...planRow(operation),
      status,
      ...(outcome?.error ? { reason: recoveryFor(outcome.error).title } : {}),
    };
  });
  const saved = rows.filter((row) => row.status === "succeeded").length;
  const total = rows.length;
  const failed = rows.find((row) => row.status === "failed");
  const details: string[] = [];

  if (saved === 0) {
    return {
      tone: "nothingSaved",
      headline:
        report.batch.stopReason === "cancelled"
          ? "Stopped before any change. No file was changed."
          : `No file was changed.${failed?.reason ? ` ${failed.reason}.` : ""}`,
      details,
      rows,
      undoable: false,
    };
  }

  if (saved < total) {
    details.push("Earlier changes were kept. You can undo them below.");
  }
  if (!report.historySettled)
    details.push("Undo may not be available for every change.");
  if (!report.indexRefreshed)
    details.push(
      "Folio's file list hasn't caught up with these changes yet. Refresh the folder to see them.",
    );

  const complete = saved === total;
  return {
    tone: complete && report.indexRefreshed ? "saved" : "partial",
    headline: complete
      ? `Saved ${changes(saved)}.`
      : report.batch.stopReason === "cancelled"
        ? `Stopped after ${saved} of ${changes(total)}.`
        : `Saved ${saved} of ${changes(total)}.${failed?.reason ? ` Stopped at ${failed.from ?? failed.to}: ${failed.reason}.` : ""}`,
    details,
    rows,
    undoable: true,
  };
}

/** Why an Undo can't go ahead, one line per blocked file. */
export function undoBlockers(preview: UndoPreflight): string[] {
  return preview.conflicts.map((conflict) => {
    switch (conflict.reason) {
      case "externallyModified":
        return `${conflict.relativePath} was changed after Folio saved it.`;
      case "missing":
        return `${conflict.relativePath} is no longer there.`;
      case "destinationOccupied":
        return `Another file is now at ${conflict.relativePath}.`;
      case "notRecoverable":
        return `Folio couldn't keep the earlier version of ${conflict.relativePath}.`;
    }
  });
}

/** The result of an Undo, from the native report alone. */
export function summarizeUndo(report: UndoReport): {
  complete: boolean;
  headline: string;
} {
  const undone = report.undoneEntryIds.length;
  const remaining = report.remainingEntryIds.length;
  if (remaining === 0 && !report.error)
    return { complete: true, headline: `Undid ${changes(undone)}.` };
  if (undone === 0)
    return {
      complete: false,
      headline: `Nothing was undone.${report.error ? ` ${recoveryFor(report.error).title}.` : ""}`,
    };
  return {
    complete: false,
    headline: `Undid ${undone} of ${changes(undone + remaining)}, then stopped. Preview Undo again to finish.`,
  };
}

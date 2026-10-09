import type {
  ActionPlan,
  ApplyReport,
  FileOperation,
  ImpactCandidate,
  OperationStatus,
  UndoPreflight,
  UndoReport,
} from "../domain/contracts";
import { recoveryFor } from "./recovery";

/** One line of an exact preview: what happens to which path. */
export interface PlanRow {
  action: "Rename" | "Move" | "Edit" | "Create" | "Delete";
  /** Absent for a new file. */
  from?: string;
  /** Absent for a deleted file. */
  to?: string;
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
    case "delete":
      return { action: "Delete", from: operation.relativePath };
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

function files(count: number): string {
  return `${count} ${count === 1 ? "file" : "files"}`;
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
  // A failure the native core reports only after the write (`historyRequired`)
  // still changed its file, so it counts as changed but not as undoable.
  let changedWithoutUndo: { row: OutcomeRow; message: string } | undefined;
  const rows: OutcomeRow[] = [];
  for (const [index, operation] of plan.operations.entries()) {
    const outcome = report.batch.outcomes.find(
      (item) => item.operationIndex === index,
    );
    const status = outcome?.status ?? "notStarted";
    const recovery = outcome?.error
      ? recoveryFor(outcome.error, "duringApply")
      : undefined;
    const row: OutcomeRow = {
      ...planRow(operation),
      status,
      ...(recovery ? { reason: recovery.title } : {}),
    };
    if (status === "failed" && recovery?.afterWrite)
      changedWithoutUndo = { row, message: recovery.message };
    rows.push(row);
  }
  const saved = rows.filter((row) => row.status === "succeeded").length;
  const changed = saved + (changedWithoutUndo ? 1 : 0);
  const total = rows.length;
  const failed = rows.find((row) => row.status === "failed");
  const details: string[] = [];

  if (changed === 0) {
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

  if (saved > 0 && saved < total) {
    details.push("Earlier changes were kept. You can undo them below.");
  }
  if (changedWithoutUndo) details.push(changedWithoutUndo.message);
  if (!report.historySettled)
    details.push("Undo may not be available for every change.");
  if (!report.indexRefreshed)
    details.push(
      "Folio's file list hasn't caught up with these changes yet. Refresh the folder to see them.",
    );

  const complete = saved === total;
  const onlyDeletions = plan.operations.every(
    (operation) => operation.kind === "delete",
  );
  return {
    tone: complete && report.indexRefreshed ? "saved" : "partial",
    headline: complete
      ? onlyDeletions
        ? `Deleted ${files(saved)}.`
        : `Saved ${changes(saved)}.`
      : report.batch.stopReason === "cancelled"
        ? `Stopped after ${changed} of ${changes(total)}.`
        : `Saved ${changed} of ${changes(total)}.${failed?.reason ? ` Stopped at ${failed.from ?? failed.to}: ${failed.reason}.` : ""}`,
    details,
    rows,
    undoable: saved > 0,
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
      headline: `Nothing was undone.${report.error ? ` ${recoveryFor(report.error, "refused").title}.` : ""}`,
    };
  return {
    complete: false,
    headline: `Undid ${undone} of ${changes(undone + remaining)}, then stopped. Preview Undo again to finish.`,
  };
}

/** Ripple candidates by how Folio knows they're related. */
export interface ImpactGroups {
  /** A link written in one of the files. */
  links: ImpactCandidate[];
  /** Byte-identical copies of the edited file, related by content alone. */
  copies: ImpactCandidate[];
  /** Found by comparing passages or suggested by a local model. */
  inferred: ImpactCandidate[];
  /** Anything that doesn't say how it's related. */
  other: ImpactCandidate[];
}

export type ImpactKind = keyof ImpactGroups;

/**
 * Links and copies are facts read from the files and are never grouped as
 * inferred. A candidate that names no relationship counts as a copy only when
 * it is also similarity-only, which is how the native core reports one.
 */
export function impactKind(impact: ImpactCandidate): ImpactKind {
  if (
    impact.relationshipType === "explicitReference" ||
    impact.provenance === "documentLink"
  )
    return "links";
  if (impact.provenance === "embedding" || impact.provenance === "model")
    return "inferred";
  if (
    !impact.relationshipType &&
    !impact.provenance &&
    impact.strength === "similarityOnly"
  )
    return "copies";
  return "other";
}

export function impactGroups(impacts: ImpactCandidate[]): ImpactGroups {
  const groups: ImpactGroups = {
    links: [],
    copies: [],
    inferred: [],
    other: [],
  };
  for (const impact of impacts) groups[impactKind(impact)].push(impact);
  return groups;
}

/** How Folio knows a candidate is related, and whether to label it AI. */
export function impactProvenance(impact: ImpactCandidate): {
  label: string;
  ai: boolean;
} {
  switch (impactKind(impact)) {
    case "links":
      return { label: "Link written in the file", ai: false };
    case "copies":
      return { label: "Same contents, byte for byte", ai: false };
    case "inferred":
      return {
        label:
          impact.provenance === "model"
            ? "Suggested by the local AI model; check before relying on it"
            : "Found by comparing passages; check before relying on it",
        ai: true,
      };
    case "other":
      return { label: "Related file", ai: false };
  }
}

/** The native core lists at most this many Ripple candidates for one plan. */
export const RIPPLE_CANDIDATE_CAP = 25;

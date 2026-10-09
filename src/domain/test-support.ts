import type {
  ActionPlan,
  ContentHash,
  FileOperation,
  FileOperationKind,
  HistoryEntry,
  RelativePath,
  PlanSource,
} from "./contracts";
import { hashText } from "./hash";
import { documentIdFor } from "./identity";
import { planDigest, type ObservedPath, type ObservedPaths } from "./plan";

/** Shared helpers for the contract and safety suites. Not shipped in the app. */
export const WORKSPACE = "a".repeat(64);

export function documentId(relativePath: string): string {
  return documentIdFor(WORKSPACE, relativePath);
}

export async function editOperation(
  relativePath: RelativePath,
  before: string,
  after: string,
): Promise<FileOperation> {
  return {
    kind: "edit",
    documentId: documentId(relativePath),
    relativePath,
    expectedContentHash: await hashText(before),
    after,
  };
}

export async function renameOperation(
  relativePath: RelativePath,
  before: string,
  destinationRelativePath: RelativePath,
): Promise<FileOperation> {
  return {
    kind: "rename",
    documentId: documentId(relativePath),
    relativePath,
    expectedContentHash: await hashText(before),
    destinationRelativePath,
    expectedDestination: "absent",
  };
}

export async function deleteOperation(
  relativePath: RelativePath,
  before: string,
): Promise<FileOperation> {
  return {
    kind: "delete",
    documentId: documentId(relativePath),
    relativePath,
    expectedContentHash: await hashText(before),
  };
}

export async function makePlan(input: {
  id: string;
  operations: FileOperation[];
  createdAt?: number;
  expiresAt?: number;
  workspaceId?: string;
  source?: PlanSource;
}): Promise<ActionPlan> {
  const plan: ActionPlan = {
    id: input.id,
    workspaceId: input.workspaceId ?? WORKSPACE,
    source: input.source ?? "organize",
    createdAt: input.createdAt ?? 1_000,
    expiresAt: input.expiresAt ?? 2_000,
    operations: input.operations,
    impacts: [],
    digest: "",
  };
  return { ...plan, digest: await planDigest(plan) };
}

export async function present(content: string): Promise<ObservedPath> {
  return { exists: true, isFile: true, contentHash: await hashText(content) };
}

export const ABSENT: ObservedPath = { exists: false, contentHash: null };

export async function observedPaths(
  entries: Record<RelativePath, string | null>,
): Promise<ObservedPaths> {
  const observed: ObservedPaths = {};
  for (const [path, content] of Object.entries(entries)) {
    observed[path] = content === null ? ABSENT : await present(content);
  }
  return observed;
}

export async function historyEntry(input: {
  id: string;
  planId: string;
  operationIndex: number;
  appliedPath: RelativePath;
  appliedContent: string;
  beforePath?: RelativePath;
  beforeContent?: string;
  recoverable?: boolean;
  operationKind?: FileOperationKind;
}): Promise<HistoryEntry> {
  const after: ContentHash = await hashText(input.appliedContent);
  const entry: HistoryEntry = {
    id: input.id,
    planId: input.planId,
    operationIndex: input.operationIndex,
    operationKind: input.operationKind ?? "edit",
    appliedAt: 1_500,
    documentId: documentId(input.appliedPath),
    afterRelativePath: input.appliedPath,
    afterContentHash: after,
    recoverable: input.recoverable ?? true,
  };
  if (input.beforePath) entry.beforeRelativePath = input.beforePath;
  if (input.beforeContent !== undefined) {
    entry.beforeContentHash = await hashText(input.beforeContent);
  }
  return entry;
}

/** A deletion's entry: no applied path and no after hash. */
export async function deletionEntry(input: {
  id: string;
  planId: string;
  path: RelativePath;
  content: string;
  recoverable?: boolean;
}): Promise<HistoryEntry> {
  return {
    id: input.id,
    planId: input.planId,
    operationIndex: 0,
    operationKind: "delete",
    appliedAt: 1_500,
    documentId: documentId(input.path),
    beforeRelativePath: input.path,
    beforeContentHash: await hashText(input.content),
    recoverable: input.recoverable ?? true,
  };
}

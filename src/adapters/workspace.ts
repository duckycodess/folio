import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  DocumentRecord,
  DuplicateGroup,
  EmbeddingSpace,
  IndexedDocument,
  IndexProgress,
  KnownWorkspace,
  NativeError,
  PendingChunk,
  Relationship,
  ScanSummary,
  SearchResult,
  VectorCandidate,
  WorkspaceInfo,
} from "../domain/contracts";

const rawFixtures = import.meta.glob("../../fixtures/documents/**/*.{md,txt}", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

export const fixtureDocuments: DocumentRecord[] = Object.entries(
  rawFixtures,
).map(([path, content]) => {
  const relativePath = path.replace("../../fixtures/documents/", "");
  const language = content.includes("Language: Filipino")
    ? "fil"
    : content.includes("Language: Taglish")
      ? "mixed"
      : "en";
  return {
    id: relativePath,
    relativePath,
    name: relativePath.split("/").at(-1)!,
    title: content.match(/^# (.+)$/m)?.[1] ?? relativePath,
    language,
    content,
    sizeBytes: new TextEncoder().encode(content).length,
  };
});

export const nativeAvailable = isTauri();

export function isNativeError(error: unknown): error is NativeError {
  return (
    typeof error === "object" &&
    error !== null &&
    typeof (error as NativeError).code === "string" &&
    typeof (error as NativeError).message === "string"
  );
}

/** Opens the native folder picker, then indexes the chosen folder before listing it. */
export async function chooseWorkspace(): Promise<{
  info: WorkspaceInfo;
  documents: IndexedDocument[];
  summary: ScanSummary;
} | null> {
  const info = await invoke<WorkspaceInfo | null>("choose_workspace");
  if (!info) return null;
  return openIndexed(info);
}

/** Folders authorized in earlier sessions; reopening one never shows a picker. */
export function listWorkspaces(): Promise<KnownWorkspace[]> {
  return invoke<KnownWorkspace[]>("list_workspaces");
}

export async function reopenWorkspace(workspaceId: string): Promise<{
  info: WorkspaceInfo;
  documents: IndexedDocument[];
  summary: ScanSummary;
}> {
  const info = await invoke<WorkspaceInfo>("reopen_workspace", { workspaceId });
  return openIndexed(info);
}

async function openIndexed(info: WorkspaceInfo) {
  const summary = await scanWorkspace(info.id);
  return { info, documents: await listDocuments(info.id), summary };
}

export function listDocuments(workspaceId: string): Promise<IndexedDocument[]> {
  return invoke<IndexedDocument[]>("list_documents", { workspaceId });
}

/** Local Sync: incrementally re-indexes the folder. Progress arrives via `onIndexProgress`. */
export function scanWorkspace(workspaceId: string): Promise<ScanSummary> {
  return invoke<ScanSummary>("scan_workspace", { workspaceId });
}

export function cancelIndexing(): Promise<void> {
  return invoke<void>("cancel_indexing");
}

export function onIndexProgress(
  handler: (progress: IndexProgress) => void,
): Promise<UnlistenFn> {
  return listen<IndexProgress>("folio://index-progress", (event) =>
    handler(event.payload),
  );
}

/** FTS5 keyword search over the persistent index; results are labelled `keyword`. */
export function searchIndex(
  workspaceId: string,
  query: string,
  limit = 20,
): Promise<SearchResult[]> {
  return invoke<SearchResult[]>("search_index", { workspaceId, query, limit });
}

export function listDuplicates(workspaceId: string): Promise<DuplicateGroup[]> {
  return invoke<DuplicateGroup[]>("list_duplicates", { workspaceId });
}

export function listRelationships(
  workspaceId: string,
): Promise<Relationship[]> {
  return invoke<Relationship[]>("list_relationships", { workspaceId });
}

/** Returns the stored space id; each distinct fingerprint is a separate space. */
export function registerEmbeddingSpace(space: EmbeddingSpace): Promise<string> {
  return invoke<string>("register_embedding_space", { space });
}

export function pendingEmbeddingChunks(
  workspaceId: string,
  spaceId: string,
  limit = 64,
): Promise<PendingChunk[]> {
  return invoke<PendingChunk[]>("pending_embedding_chunks", {
    workspaceId,
    spaceId,
    limit,
  });
}

export function putEmbeddings(
  workspaceId: string,
  spaceId: string,
  items: { chunkId: number; vector: number[] }[],
): Promise<number> {
  return invoke<number>("put_embeddings", { workspaceId, spaceId, items });
}

export function vectorCandidates(
  workspaceId: string,
  spaceId: string,
  vector: number[],
  k = 20,
): Promise<VectorCandidate[]> {
  return invoke<VectorCandidate[]>("vector_candidates", {
    workspaceId,
    spaceId,
    vector,
    k,
  });
}

export async function readNativeDocument(
  workspaceId: string,
  document: DocumentRecord,
): Promise<DocumentRecord> {
  const content = await invoke<string>("read_document", {
    workspaceId,
    relativePath: document.relativePath,
  });
  return {
    ...document,
    content,
    title: content.match(/^# (.+)$/m)?.[1] ?? document.name,
  };
}

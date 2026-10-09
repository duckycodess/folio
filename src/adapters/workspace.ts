import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  FIXTURE_WORKSPACE_ID,
  type ContentHash,
  type DocumentId,
  type DocumentRecord,
  type DuplicateGroup,
  type EmbeddingSyncSummary,
  type EmbeddingSpace,
  type EmbeddingSpaceFingerprint,
  type FolioErrorCode,
  type IndexedDocument,
  type IndexProgress,
  type KnownWorkspace,
  type Language,
  type MediaType,
  type PendingChunk,
  type Relationship,
  type ScanSummary,
  type SearchResult,
  type VectorCandidate,
  type WorkspaceId,
  type WorkspaceInfo,
} from "../domain/contracts";
import { toFolioError } from "../domain/errors";
import { hashText } from "../domain/hash";
import { documentIdFor, mediaTypeForPath } from "../domain/identity";
import { validateRelationship } from "../domain/relationships";

const rawFixtures = import.meta.glob("../../fixtures/documents/**/*.{md,txt}", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

function fixtureLanguage(content: string): Language {
  if (content.includes("Language: Filipino")) return "fil";
  if (content.includes("Language: Taglish")) return "mixed";
  return "en";
}

/**
 * The in-repo development corpus. These are synthetic documents, not a user's
 * files, and they are hashed exactly like real ones so evidence produced from
 * them carries a real revision.
 */
export async function loadFixtureDocuments(): Promise<DocumentRecord[]> {
  const entries = Object.entries(rawFixtures).sort(([a], [b]) =>
    a.localeCompare(b),
  );
  return Promise.all(
    entries.map(async ([path, content]) => {
      const relativePath = path.replace("../../fixtures/documents/", "");
      return {
        id: documentIdFor(FIXTURE_WORKSPACE_ID, relativePath),
        workspaceId: FIXTURE_WORKSPACE_ID,
        relativePath,
        name: relativePath.split("/").at(-1)!,
        title: content.match(/^# (.+)$/m)?.[1] ?? relativePath,
        language: fixtureLanguage(content),
        mediaType: mediaTypeForPath(relativePath) ?? "text/plain",
        sizeBytes: new TextEncoder().encode(content).length,
        contentHash: await hashText(content),
        content,
      } satisfies DocumentRecord;
    }),
  );
}

export const nativeAvailable = isTauri();

/** One row of `list_documents`; see `src-tauri/src/workspace.rs`. */
interface NativeDocument {
  id: string;
  workspaceId: string;
  relativePath: string;
  name: string;
  mediaType: MediaType;
  sizeBytes: number;
  modifiedAtMs: number | null;
}

/** A file the native core found but will not present as a document. */
interface NativeSkippedEntry {
  displayName: string;
  code: FolioErrorCode;
}

/** Exactly what `list_documents` returns. */
interface NativeListing {
  workspaceId: string;
  documents: NativeDocument[];
  skipped: NativeSkippedEntry[];
}

/** Exactly what `read_document` returns. */
interface NativeDocumentText {
  content: string;
  contentHash: ContentHash;
  sizeBytes: number;
  modifiedAtMs: number | null;
  /** PDFs only: each page's UTF-8 byte range in `content`. */
  pages?: { page: number; start: number; end: number }[];
  /** PDFs only: pages whose text couldn't be extracted. */
  unreadablePages?: number[];
}

function toRecord(row: NativeDocument): DocumentRecord {
  const record: DocumentRecord = {
    id: row.id,
    workspaceId: row.workspaceId,
    relativePath: row.relativePath,
    name: row.name,
    title: row.name,
    language: "unknown",
    mediaType: row.mediaType,
    sizeBytes: row.sizeBytes,
  };
  if (row.modifiedAtMs !== null) record.modifiedAtMs = row.modifiedAtMs;
  return record;
}

export interface ChosenWorkspace {
  info: WorkspaceInfo;
  documents: DocumentRecord[];
  /** Files Folio could not identify. Reported rather than quietly dropped. */
  skipped: NativeSkippedEntry[];
}

export async function chooseWorkspace(): Promise<ChosenWorkspace | null> {
  try {
    const info = await invoke<WorkspaceInfo | null>("choose_workspace");
    if (!info) return null;
    return { info, ...(await listFolder(info.id)) };
  } catch (cause) {
    throw toFolioError(cause);
  }
}

/** Lists an authorized folder again, for example after Folio changed it. */
export async function listFolder(
  workspaceId: string,
): Promise<Omit<ChosenWorkspace, "info">> {
  try {
    const listing = await invoke<NativeListing>("list_documents", {
      workspaceId,
    });
    return {
      documents: listing.documents.map(toRecord),
      skipped: listing.skipped,
    };
  } catch (cause) {
    throw toFolioError(cause);
  }
}

export async function readNativeDocument(
  workspaceId: string,
  document: DocumentRecord,
): Promise<DocumentRecord> {
  try {
    const read = await invoke<NativeDocumentText>("read_document", {
      workspaceId,
      relativePath: document.relativePath,
    });
    return {
      ...document,
      content: read.content,
      contentHash: read.contentHash,
      sizeBytes: read.sizeBytes,
      ...(read.modifiedAtMs !== null
        ? { modifiedAtMs: read.modifiedAtMs }
        : {}),
      title: read.content.match(/^# (.+)$/m)?.[1] ?? document.name,
      ...(read.pages?.length ? { pages: read.pages } : {}),
      ...(read.unreadablePages?.length
        ? { unreadablePages: read.unreadablePages }
        : {}),
    };
  } catch (cause) {
    throw toFolioError(cause);
  }
}

/* -------------------------------------------------------- persistent index */

async function call<T>(command: string, args?: Record<string, unknown>) {
  try {
    return await invoke<T>(command, args);
  } catch (cause) {
    throw toFolioError(cause);
  }
}

/** Folders authorized in earlier sessions; restoring one never shows a picker. */
export function listWorkspaces(): Promise<KnownWorkspace[]> {
  return call<KnownWorkspace[]>("list_workspaces");
}

export function reopenWorkspace(
  workspaceId: WorkspaceId,
): Promise<WorkspaceInfo> {
  return call<WorkspaceInfo>("reopen_workspace", { workspaceId });
}

/**
 * Local Sync: incrementally re-indexes the folder. Progress arrives via
 * `onIndexProgress`. `recheckUnreadable` also reads every failed or stale
 * document that is waiting before Folio checks it again.
 */
export function scanWorkspace(
  workspaceId: WorkspaceId,
  options: { recheckUnreadable?: boolean } = {},
): Promise<ScanSummary> {
  return call<ScanSummary>("scan_workspace", {
    workspaceId,
    recheckUnreadable: options.recheckUnreadable ?? false,
  });
}

/** "Check again": reads these documents now and returns their updated records. */
export function recheckDocuments(
  workspaceId: WorkspaceId,
  documentIds: DocumentId[],
): Promise<IndexedDocument[]> {
  return call<IndexedDocument[]>("recheck_documents", {
    workspaceId,
    documentIds,
  });
}

export function cancelIndexing(): Promise<void> {
  return call<void>("cancel_indexing");
}

export function onIndexProgress(
  handler: (progress: IndexProgress) => void,
): Promise<UnlistenFn> {
  return listen<IndexProgress>("folio://index-progress", (event) =>
    handler(event.payload),
  );
}

export function listIndexedDocuments(
  workspaceId: WorkspaceId,
): Promise<IndexedDocument[]> {
  return call<IndexedDocument[]>("list_indexed_documents", { workspaceId });
}

/** FTS5 keyword search over the persistent index; results are labelled `keyword`. */
export function searchIndex(
  workspaceId: WorkspaceId,
  query: string,
  limit = 20,
): Promise<SearchResult[]> {
  return call<SearchResult[]>("search_index", { workspaceId, query, limit });
}

export function listDuplicates(
  workspaceId: WorkspaceId,
): Promise<DuplicateGroup[]> {
  return call<DuplicateGroup[]>("list_duplicates", { workspaceId });
}

export interface RelationshipListing {
  relationships: Relationship[];
  /** Rows rejected at the UI boundary because their evidence was malformed. */
  invalidCount: number;
}

export async function listRelationshipsWithDiagnostics(
  workspaceId: WorkspaceId,
  spaceFingerprint?: EmbeddingSpaceFingerprint,
): Promise<RelationshipListing> {
  const rows = await call<Relationship[]>("list_relationships", {
    workspaceId,
    spaceFingerprint,
  });
  const relationships: Relationship[] = [];
  let invalidCount = 0;
  for (const row of rows) {
    try {
      validateRelationship(row);
      relationships.push(row);
    } catch {
      invalidCount += 1;
    }
  }
  return { relationships, invalidCount };
}

export async function listRelationships(
  workspaceId: WorkspaceId,
  spaceFingerprint?: EmbeddingSpaceFingerprint,
): Promise<Relationship[]> {
  return (await listRelationshipsWithDiagnostics(workspaceId, spaceFingerprint))
    .relationships;
}

/** Returns the space fingerprint; vectors are compared only within one space. */
export function registerEmbeddingSpace(
  space: EmbeddingSpace,
): Promise<EmbeddingSpaceFingerprint> {
  return call<EmbeddingSpaceFingerprint>("register_embedding_space", {
    space,
  });
}

export function pendingEmbeddingChunks(
  workspaceId: WorkspaceId,
  spaceFingerprint: EmbeddingSpaceFingerprint,
  limit = 64,
): Promise<PendingChunk[]> {
  return call<PendingChunk[]>("pending_embedding_chunks", {
    workspaceId,
    spaceFingerprint,
    limit,
  });
}

export function putEmbeddings(
  workspaceId: WorkspaceId,
  spaceFingerprint: EmbeddingSpaceFingerprint,
  items: { chunkId: number; contentHash: ContentHash; vector: number[] }[],
): Promise<number> {
  return call<number>("put_embeddings", {
    workspaceId,
    spaceFingerprint,
    items,
  });
}

export function vectorCandidates(
  workspaceId: WorkspaceId,
  spaceFingerprint: EmbeddingSpaceFingerprint,
  vector: number[],
  k = 20,
): Promise<VectorCandidate[]> {
  return call<VectorCandidate[]>("vector_candidates", {
    workspaceId,
    spaceFingerprint,
    vector,
    k,
  });
}

/** Fill the persistent store from pending chunks using the installed local model. */
export function syncEmbeddings(
  workspaceId: WorkspaceId,
): Promise<EmbeddingSyncSummary> {
  return call<EmbeddingSyncSummary>("sync_embeddings", { workspaceId });
}

/** Stop a persistent embedding fill after the current provider operation. */
export function cancelEmbeddingSync(): Promise<void> {
  return call<void>("cancel_embedding_sync");
}

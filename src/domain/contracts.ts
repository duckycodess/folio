export type DocumentId = string;
export type Language = "en" | "fil" | "mixed" | "unknown";
export type RelationshipType =
  "similarity" | "explicitReference" | "sharedFactCandidate";

export interface DocumentRecord {
  id: DocumentId;
  relativePath: string;
  name: string;
  title: string;
  language: Language;
  sizeBytes: number;
  content?: string;
  contentHash?: string;
}

/**
 * `start`/`end` are UTF-16 code-unit offsets (JavaScript string indices) into the
 * document's extracted text, so `content.slice(start, end) === text`. For PDFs the
 * extracted text is the pages joined by a blank line; `page` is 1-based.
 */
export interface SourcePassage {
  documentId: DocumentId;
  start: number;
  end: number;
  text: string;
  page?: number;
}

export interface Relationship {
  sourceId: DocumentId;
  targetId: DocumentId;
  type: RelationshipType;
  evidence: SourcePassage[];
  provenance: "documentLink" | "model" | "embedding";
  confidence?: number;
  sourceContentHash?: string;
  targetContentHash?: string;
}

export interface SearchResult {
  document: DocumentRecord;
  passages: SourcePassage[];
  score: number;
  method: "keyword" | "semantic" | "hybrid";
}

export interface GroundedAnswer {
  text: string;
  sources: SourcePassage[];
  coverage: DocumentId[];
  modelId: string;
}

export interface EmbeddingSpace {
  modelId: string;
  revision: string;
  quantization: string;
  dimensions: number;
  preprocessingFingerprint: string;
}

export interface EmbeddingProvider {
  space: EmbeddingSpace;
  embed(texts: string[], signal?: AbortSignal): Promise<number[][]>;
  unload(): Promise<void>;
}

export interface GenerationProvider {
  modelId: string;
  revision: string;
  generate(request: {
    instruction: string;
    sources: SourcePassage[];
    outputLanguage: Language;
    maxOutputTokens: number;
    signal?: AbortSignal;
  }): Promise<GroundedAnswer>;
  unload(): Promise<void>;
}

export type FileOperation =
  | {
      kind: "edit";
      documentId: DocumentId;
      expectedContentHash: string;
      before: string;
      after: string;
    }
  | {
      kind: "rename" | "move";
      documentId: DocumentId;
      expectedContentHash: string;
      destinationRelativePath: string;
    }
  | { kind: "create"; destinationRelativePath: string; content: string };

/**
 * Folio Ripple review candidate. Never an operation: candidates are not changed.
 * `evidence`: links to/from the target and mentions the replaced value.
 * `similarityOnly`: related another way (e.g. a byte-identical copy); no claim it must change.
 */
export interface ImpactCandidate {
  documentId: DocumentId;
  /** Present on native previews. */
  relativePath?: string;
  reason: string;
  evidence: SourcePassage[];
  strength: "evidence" | "similarityOnly";
}

export interface ActionPlan {
  id: string;
  workspaceId: string;
  operations: FileOperation[];
  impacts: ImpactCandidate[];
  createdAt: number;
  expiresAt: number;
}

export interface WorkspaceInfo {
  id: string;
  rootPath: string;
}

/** A folder previously authorized through the native picker. */
export interface KnownWorkspace extends WorkspaceInfo {
  authorizedAt: string;
  lastOpenedAt: string | null;
  /** False when the folder is gone or Folio lost permission to read it. */
  available: boolean;
}

export type NativeErrorCode =
  | "PATH_ESCAPE"
  | "NOT_AUTHORIZED"
  | "NOT_FOUND"
  | "UNSUPPORTED"
  | "TOO_LARGE"
  | "INVALID_INPUT"
  | "BUSY"
  | "EMBEDDING_SPACE_MISMATCH"
  | "IO"
  | "DATABASE"
  | "TARGET_CHANGED"
  | "STALE_INDEX"
  | "AMBIGUOUS_EDIT"
  | "UNSUPPORTED_EDIT"
  | "COLLISION"
  | "APPROVAL_REQUIRED"
  | "PLAN_EXPIRED"
  | "PLAN_CHANGED"
  | "PLAN_STATE"
  | "UNDO_CONFLICT"
  | "UNDO_UNAVAILABLE";

/** Every native command rejects with this shape. */
export interface NativeError {
  code: NativeErrorCode;
  message: string;
}

export type MediaType = "text/plain" | "text/markdown" | "application/pdf";

/**
 * `indexed`: current content is searchable. `unsupported`: readable but not indexable
 * (e.g. a scanned PDF without a text layer). `failed`: never indexed successfully.
 * `stale`: the file changed but could not be re-read; search shows the previous version.
 */
export type IndexStatus = "indexed" | "unsupported" | "failed" | "stale";

/** A document as recorded by the native index. `id` is stable across Folio renames/moves. */
export interface IndexedDocument extends DocumentRecord {
  contentHash: string;
  mediaType: MediaType;
  status: IndexStatus;
  statusMessage?: string;
  modifiedAt: string;
  indexedAt: string | null;
}

export interface IndexProgress {
  workspaceId: string;
  phase: "discovering" | "indexing" | "linking" | "done" | "cancelled";
  processed: number;
  total: number;
  currentPath?: string;
}

/** Counts describe one scan; `unchanged` documents were not re-extracted. */
export interface ScanSummary {
  workspaceId: string;
  total: number;
  added: number;
  updated: number;
  unchanged: number;
  removed: number;
  unsupported: number;
  failed: number;
  stale: number;
  skipped: number;
  cancelled: boolean;
  durationMs: number;
}

/** Documents whose bytes were re-read and found identical; not a similarity judgement. */
export interface DuplicateGroup {
  contentHash: string;
  sizeBytes: number;
  documents: IndexedDocument[];
}

/** A chunk without a vector in the given embedding space. */
export interface PendingChunk {
  chunkId: number;
  documentId: DocumentId;
  text: string;
}

/** Exact cosine match within a single embedding space. */
export interface VectorCandidate {
  chunkId: number;
  score: number;
  passage: SourcePassage;
}

export interface BenchmarkResult {
  caseId: string;
  task: "retrieval" | "interpretation" | "summary" | "edit";
  modelId: string;
  revision: string;
  quantization: string;
  runtime: string;
  hardware: string;
  contextTokens: number;
  cold: boolean;
  taskDurationMs: number;
  correctness: boolean | null;
  peakProcessRamBytes: number | null;
  modelDiskBytes: number;
}

/**
 * Native edit semantics: `before` must occur exactly once in the current file and is
 * replaced by `after`. Rename keeps the folder; move needs an existing destination folder.
 */
export interface OperationDiff {
  kind: FileOperation["kind"];
  documentId: DocumentId | null;
  sourcePath: string | null;
  destinationPath: string;
  /** The affected line before and after an edit; the opening of a created file. */
  beforeExcerpt: string | null;
  afterExcerpt: string | null;
}

export type PlanStatus =
  "preview" | "approved" | "applied" | "expired" | "failed";

/** A native, expiring preview. Approval must echo `digest`; previews are not saved files. */
export interface PlanPreview extends ActionPlan {
  diffs: OperationDiff[];
  digest: string;
  status: PlanStatus;
  statusMessage: string | null;
}

export interface RevertReport {
  operationIndex: number;
  path: string;
  reverted: boolean;
  message: string | null;
}

/** `failed` lists which earlier writes were reverted; nothing is claimed as atomic. */
export interface ApplyResult {
  planId: string;
  status: "applied" | "failed";
  message: string | null;
  failedOperationIndex: number | null;
  reverts: RevertReport[];
  indexRefreshed: boolean;
}

export interface UndoResult {
  planId: string;
  status: "undone" | "partial";
  message: string | null;
  undoneOperations: number[];
  indexRefreshed: boolean;
}

export interface HistoryOperation {
  index: number;
  kind: FileOperation["kind"];
  documentId: DocumentId | null;
  beforePath: string | null;
  afterPath: string | null;
  beforeHash: string | null;
  afterHash: string | null;
  undoneAt: string | null;
}

/** One applied plan. Older entries stay listed after their recoverable content is pruned. */
export interface HistoryEntry {
  planId: string;
  appliedAt: string | null;
  undoable: boolean;
  undone: boolean;
  operations: HistoryOperation[];
}

export interface FilenameSuggestion {
  documentId: DocumentId;
  relativePath: string;
  suggestedRelativePath: string;
  contentHash: string;
  reason: string;
}

/** Duplicates are evidence only; a filename suggestion becomes a rename plan when chosen. */
export interface OrganizeSuggestions {
  duplicates: DuplicateGroup[];
  filenames: FilenameSuggestion[];
}

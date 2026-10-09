/**
 * Folio cross-track boundary types.
 *
 * Every track (UI, workspace/indexing, providers, actions) uses these names and
 * shapes. Native Rust serializes the same fields in camelCase. The companion
 * reference is `docs/contracts.md`; change it and this file together, and
 * announce the change before merging.
 *
 * This file contains types only. Deterministic logic lives beside it in
 * `errors.ts`, `identity.ts`, `offsets.ts`, `relationships.ts`, `plan.ts` and
 * `approval.ts` so that both languages can be checked against the same golden
 * fixtures in `fixtures/contracts/`.
 */

/* ------------------------------------------------------------------ errors */

/**
 * Every failure crossing the boundary carries one of these codes. Native
 * commands return the code; user-facing prose is carried separately and is
 * never the thing a caller branches on.
 */
export type FolioErrorCode =
  // Workspace authorization and path containment.
  | "workspaceNotAuthorized"
  | "workspaceUnavailable"
  | "pathNotRelative"
  | "pathEscapesWorkspace"
  | "pathUnsupportedEncoding"
  | "documentUnavailable"
  | "documentTooLarge"
  | "documentNotText"
  | "unsupportedMediaType"
  // Plans, approval and durable outcomes.
  | "planUnknown"
  | "planEmpty"
  | "planExpired"
  | "planStateInvalid"
  | "planDigestMismatch"
  | "approvalRequired"
  | "approvalStale"
  | "duplicateOperationTarget"
  | "targetMissing"
  | "targetChanged"
  | "destinationExists"
  | "operationUnsupported"
  | "historyRequired"
  | "historyUnknown"
  | "undoConflict"
  | "writerNotImplemented"
  // Providers.
  | "modelNotInstalled"
  | "modelLoadFailed"
  | "providerBusy"
  | "cancelled"
  | "contextOverflow"
  // Retrieval and evidence.
  | "embeddingSpaceMismatch"
  | "evidenceInvalid"
  // Anything the caller cannot act on specifically.
  | "internal";

/**
 * Context for a failure, as a flat map of strings. Numbers and absent values are
 * stringified where they are reported, so this and the native
 * `BTreeMap<String, String>` carry exactly the same thing. Keys are documented
 * per code in docs/contracts.md.
 */
export type FolioErrorDetails = Record<string, string>;

/** The wire form of a failure. Native `Result::Err` serializes exactly this. */
export interface FolioErrorPayload {
  code: FolioErrorCode;
  /** English prose for the user. Never parsed by callers. */
  message: string;
  details?: FolioErrorDetails;
}

/**
 * Every code, in one runtime list, so the native enum and this union can be
 * checked against the same fixture. `MissingErrorCode` is `never` only while
 * the list is complete, so adding a code without listing it fails `tsc`.
 */
export const FOLIO_ERROR_CODES = [
  "workspaceNotAuthorized",
  "workspaceUnavailable",
  "pathNotRelative",
  "pathEscapesWorkspace",
  "pathUnsupportedEncoding",
  "documentUnavailable",
  "documentTooLarge",
  "documentNotText",
  "unsupportedMediaType",
  "planUnknown",
  "planEmpty",
  "planExpired",
  "planStateInvalid",
  "planDigestMismatch",
  "approvalRequired",
  "approvalStale",
  "duplicateOperationTarget",
  "targetMissing",
  "targetChanged",
  "destinationExists",
  "operationUnsupported",
  "historyRequired",
  "historyUnknown",
  "undoConflict",
  "writerNotImplemented",
  "modelNotInstalled",
  "modelLoadFailed",
  "providerBusy",
  "cancelled",
  "contextOverflow",
  "embeddingSpaceMismatch",
  "evidenceInvalid",
  "internal",
] as const satisfies readonly FolioErrorCode[];

export type MissingErrorCode = Exclude<
  FolioErrorCode,
  (typeof FOLIO_ERROR_CODES)[number]
>;

const _allErrorCodesListed: MissingErrorCode extends never ? true : false =
  true;
void _allErrorCodesListed;

/** Codes a provider adapter is allowed to reject with. */
export const PROVIDER_ERROR_CODES = [
  "modelNotInstalled",
  "modelLoadFailed",
  "providerBusy",
  "cancelled",
  "contextOverflow",
] as const satisfies readonly FolioErrorCode[];

/* ---------------------------------------------------------------- identity */

/**
 * A workspace identity is stable for the same canonical root folder across
 * restarts, so a restored preview can be bound back to the folder it came from.
 * The native core derives it from the canonical root path; it never contains
 * `:`, which keeps document identities unambiguous.
 */
export type WorkspaceId = string;

/**
 * `${WorkspaceId}:${RelativePath}`. Reversible, never lossy, and stable for the
 * same file in the same authorized folder. It is not derived from file
 * contents, so an edit does not change a document's identity.
 */
export type DocumentId = string;

/**
 * A `/`-separated, NFC-normalized path below an authorized root. It never
 * starts with `/`, never contains `\`, `.`, `..`, control characters or empty
 * segments, and is rejected rather than lossily converted when the operating
 * system path is not valid Unicode.
 */
export type RelativePath = string;

/** Reserved workspace identity for the in-repo development fixture corpus. */
export const FIXTURE_WORKSPACE_ID: WorkspaceId = "fixtures";

export interface WorkspaceInfo {
  id: WorkspaceId;
  /** Canonical absolute path of the authorized folder, for display. */
  rootPath: string;
  /** Epoch milliseconds when the user authorized this folder. */
  authorizedAt: number;
}

/* ------------------------------------------------------------ content hash */

export type HashAlgorithm = "sha256";

/** `sha256:<64 lowercase hex>` over the exact bytes of a file or payload. */
export type ContentHash = string;

export const HASH_ALGORITHM: HashAlgorithm = "sha256";

/* ------------------------------------------------------------------- media */

/** Formats Folio identifies. Content editing is limited to the two text types. */
export type MediaType = "text/plain" | "text/markdown" | "application/pdf";

/** Media types a write operation may produce. PDFs are read/index-only. */
export const EDITABLE_MEDIA_TYPES = [
  "text/plain",
  "text/markdown",
] as const satisfies readonly MediaType[];

export type EditableMediaType = (typeof EDITABLE_MEDIA_TYPES)[number];

export type Language = "en" | "fil" | "mixed" | "unknown";

/* --------------------------------------------------------------- documents */

export interface DocumentRecord {
  id: DocumentId;
  workspaceId: WorkspaceId;
  relativePath: RelativePath;
  /** Final path segment, including the extension. */
  name: string;
  title: string;
  language: Language;
  mediaType: MediaType;
  /**
   * Exact byte length of the file on disk — never a character count and never a
   * UTF-16 length. For in-memory fixture documents it is the UTF-8 byte length
   * of `content`.
   */
  sizeBytes: number;
  /** Filesystem modification time in epoch milliseconds, when known. */
  modifiedAtMs?: number;
  /**
   * Hash of the exact file bytes. Absent until the document has been read;
   * listing a folder does not read file contents.
   */
  contentHash?: ContentHash;
  /** Decoded UTF-8 text, present only once the document has been read. */
  content?: string;
}

/* -------------------------------------------------------- source passages */

/**
 * The only offset unit on the boundary. Offsets are UTF-8 byte offsets into the
 * decoded document text, so Rust and TypeScript agree on Filipino, Taglish and
 * any other non-ASCII content. A union of one keeps a future change explicit.
 */
export type OffsetUnit = "utf8Byte";

export const SOURCE_OFFSET_UNIT: OffsetUnit = "utf8Byte";

export interface SourcePassage {
  documentId: DocumentId;
  /** The document revision the offsets refer to. Stale evidence is detectable. */
  documentContentHash: ContentHash;
  offsetUnit: OffsetUnit;
  /** Inclusive UTF-8 byte offset, on a character boundary. */
  start: number;
  /** Exclusive UTF-8 byte offset, on a character boundary. */
  end: number;
  /** The excerpt itself, exactly the bytes between `start` and `end`. */
  text: string;
  /** 1-based page number for paged media such as PDF. Absent for TXT/Markdown. */
  page?: number;
}

/* ------------------------------------------------------------- retrieval */

export type RetrievalMethod = "keyword" | "semantic" | "hybrid";

export interface SearchResult {
  document: DocumentRecord;
  passages: SourcePassage[];
  score: number;
  method: RetrievalMethod;
  /** Present for `semantic`/`hybrid`; identifies the compared vector space. */
  spaceFingerprint?: EmbeddingSpaceFingerprint;
}

/* --------------------------------------------------------- relationships */

export type RelationshipType =
  "similarity" | "explicitReference" | "sharedFactCandidate";

export type RelationshipProvenance = "documentLink" | "embedding" | "model";

interface RelationshipBase {
  sourceId: DocumentId;
  targetId: DocumentId;
  /** Revision of the source document the evidence was taken from. */
  sourceContentHash: ContentHash;
  /** Revision of the target document the evidence was taken from. */
  targetContentHash: ContentHash;
}

/**
 * A relationship always carries evidence typed for its kind. Evidence is
 * document text; it never authorizes a file operation.
 */
export type Relationship =
  | (RelationshipBase & {
      type: "explicitReference";
      provenance: "documentLink";
      /** The link as written, and the path it resolved to inside the workspace. */
      link: { rawTarget: string; resolvedRelativePath: RelativePath };
      /** At least one passage, located in the source document. */
      evidence: SourcePassage[];
    })
  | (RelationshipBase & {
      type: "similarity";
      provenance: "embedding";
      /** Vectors from different spaces are never compared. */
      spaceFingerprint: EmbeddingSpaceFingerprint;
      /** Similarity in [0, 1]. Not a claim that an edit must propagate. */
      score: number;
      /** The compared passages: at least one in each document. */
      sourceEvidence: SourcePassage[];
      targetEvidence: SourcePassage[];
    })
  | (RelationshipBase & {
      type: "sharedFactCandidate";
      provenance: "embedding" | "model";
      /** Passages that may state the same fact: at least one in each document. */
      sourceEvidence: SourcePassage[];
      targetEvidence: SourcePassage[];
      /** Optional confidence in [0, 1]. A candidate is never a confirmed contradiction. */
      confidence?: number;
    });

/* ------------------------------------------------------------- embeddings */

export interface EmbeddingSpace {
  modelId: string;
  revision: string;
  quantization: string;
  dimensions: number;
  /** Identifies query/passage prefixes, truncation and normalization. */
  preprocessingFingerprint: string;
}

/** Canonical single-string form of an `EmbeddingSpace`; see `identity.ts`. */
export type EmbeddingSpaceFingerprint = string;

/* -------------------------------------------------------------- providers */

/**
 * Adapters reject with a `FolioError` carrying a `PROVIDER_ERROR_CODES` code.
 * Aborting `signal` rejects with `cancelled`. Only one generative request runs
 * at a time; a second concurrent request rejects with `providerBusy`.
 */
export interface EmbeddingProvider {
  space: EmbeddingSpace;
  fingerprint: EmbeddingSpaceFingerprint;
  embed(texts: string[], signal?: AbortSignal): Promise<number[][]>;
  unload(): Promise<void>;
}

export interface GenerationRequest {
  instruction: string;
  /** Bounded retrieved evidence. The model never receives the whole corpus. */
  sources: SourcePassage[];
  outputLanguage: Language;
  maxOutputTokens: number;
  signal?: AbortSignal;
}

export type GroundedAnswerKind =
  "fileSummary" | "partialSummary" | "answer" | "insufficientEvidence";

export interface GroundedSentence {
  text: string;
  citations: SourcePassage[];
}

export interface CoverageRange {
  start: number;
  end: number;
}

export interface CoverageEntry {
  documentId: DocumentId;
  documentContentHash: ContentHash;
  offsetUnit: OffsetUnit;
  ranges: CoverageRange[];
  complete: boolean;
}

export interface GroundedAnswer {
  text: string;
  sources: SourcePassage[];
  /** Documents the answer claims to cover — retrieved excerpts, not the corpus. */
  coverage: DocumentId[];
  modelId: string;
  revision: string;
}

/**
 * Issue #4's additive result shape. It remains structurally assignable to the
 * frozen #2 GroundedAnswer while preserving sentence-level evidence details.
 */
export interface GroundedResult extends GroundedAnswer {
  kind: GroundedAnswerKind;
  sentences: GroundedSentence[];
  coverageRanges: CoverageEntry[];
  uncitedSentenceCount: number;
}

/** A model run either answers from evidence or reports that it has none. */
export type GenerationOutcome =
  | { kind: "answer"; answer: GroundedAnswer }
  | {
      kind: "insufficientEvidence";
      inspected: DocumentId[];
      message: string;
    };

export interface GenerationProvider {
  modelId: string;
  revision: string;
  quantization: string;
  runtime: string;
  generate(request: GenerationRequest): Promise<GenerationOutcome>;
  unload(): Promise<void>;
}

/* ---------------------------------------------------------- issue #4 models */

export type ModelRole = "embedding" | "generation";

export interface ModelFile {
  path: string;
  sha256: string;
  bytes: number;
  downloadUrl?: string;
}

export interface ModelDescriptor {
  id: string;
  role: ModelRole;
  repo: string;
  revision: string;
  files: ModelFile[];
  quantization: string;
  license: string;
  runtime: string;
  optionalPack: boolean;
}

export type ModelInstallStatus =
  "notInstalled" | "downloading" | "verifying" | "installed" | "corrupt";

export interface ModelInstallState {
  id: string;
  status: ModelInstallStatus;
  modelFileBytes?: number;
  error?: FolioErrorPayload;
}

export interface RuntimeStatus {
  id: string;
  version: string;
  installed: boolean;
  executablePath?: string;
}

export interface SkippedDocument {
  relativePath: string;
  reason: string;
}

/** Status of the issue #4 provider's interim in-memory retrieval snapshot. */
export interface ProviderIndexStatus {
  workspaceId?: string;
  documentCount: number;
  chunkCount: number;
  method: "keyword" | "semantic" | "hybrid";
  spaceFingerprint?: EmbeddingSpaceFingerprint;
  skippedDocuments?: SkippedDocument[];
}

/* -------------------------------------------------------------- operations */

export type FileOperationKind = "create" | "edit" | "rename" | "move";

/**
 * `expectedDestination: "absent"` is the explicit destination-absence check: a
 * rename or move is refused when something already occupies the destination,
 * rather than overwriting it.
 */
export type FileOperation =
  | {
      kind: "create";
      destinationRelativePath: RelativePath;
      mediaType: EditableMediaType;
      content: string;
      expectedDestination: "absent";
    }
  | {
      kind: "edit";
      documentId: DocumentId;
      relativePath: RelativePath;
      /** The revision the preview was built from. */
      expectedContentHash: ContentHash;
      /** Full replacement text. The current file is pinned by the hash above. */
      after: string;
    }
  | {
      kind: "rename" | "move";
      documentId: DocumentId;
      relativePath: RelativePath;
      expectedContentHash: ContentHash;
      destinationRelativePath: RelativePath;
      expectedDestination: "absent";
    };

/** A Folio Ripple review candidate. It is never written to. */
export interface ImpactCandidate {
  documentId: DocumentId;
  relativePath: RelativePath;
  reason: string;
  evidence: SourcePassage[];
  strength: "evidence" | "similarityOnly";
  /**
   * The relationship that connected this candidate to the target, when one
   * did. Absent for a byte-identical copy, which is related by content alone.
   */
  relationshipType?: RelationshipType;
  provenance?: RelationshipProvenance;
}

export interface ActionPlan {
  /** Issued by the native core. The UI cannot mint a plan identity. */
  id: string;
  workspaceId: WorkspaceId;
  /** Epoch milliseconds. */
  createdAt: number;
  /** Epoch milliseconds. Approval and application both re-check this. */
  expiresAt: number;
  /** At least one operation; order is the application order. */
  operations: FileOperation[];
  /** Review candidates shown with the preview. Excluded from the digest. */
  impacts: ImpactCandidate[];
  /** `sha256` over the canonical plan bytes; see `plan.ts`. */
  digest: ContentHash;
}

/** A proposal is display-only until the native issue #2/#5 plan boundary accepts it. */
export type OperationProposal =
  | {
      kind: "edit";
      documentId: DocumentId;
      relativePath: RelativePath;
      observedContentHash: ContentHash;
      find: string;
      replace: string;
      targetEvidence: SourcePassage;
    }
  | {
      kind: "rename" | "move";
      documentId: DocumentId;
      relativePath: RelativePath;
      observedContentHash: ContentHash;
      destinationRelativePath: RelativePath;
    }
  | {
      kind: "create";
      destinationRelativePath: RelativePath;
      content: string;
    };

export type InterpretationResult =
  | {
      status: "proposal";
      proposal: OperationProposal;
      requestLanguage: Language;
      exactDuplicatePaths?: RelativePath[];
    }
  | {
      status: "needsFileSelection";
      candidates: SearchResult[];
      pendingIntent: string;
    }
  | {
      status: "needsClarification";
      question: string;
      reason: string;
    }
  | {
      status: "nonMutating";
      intent: "search" | "summarize" | "question";
      targetQuery?: string;
    }
  | { status: "unsupported"; reason: string }
  | { status: "invalidModelOutput"; rawOutputDigest: string };

/* --------------------------------------------------- approval and outcomes */

/**
 * Approval binds to one plan identity *and* its digest. A plan whose operations
 * changed produces a different digest, so the old approval no longer applies.
 */
export interface Approval {
  planId: string;
  planDigest: ContentHash;
  approvedAt: number;
}

/**
 * Durable per-operation outcome.
 * - `succeeded`: the file changed and a history entry records how to undo it.
 * - `failed`: this operation stopped the batch; earlier successes are kept.
 * - `cancelled`: not started because the user cancelled after saving began.
 * - `notStarted`: not started because an earlier operation failed.
 */
export const OPERATION_STATUSES = [
  "succeeded",
  "failed",
  "cancelled",
  "notStarted",
] as const;

export type OperationStatus = (typeof OPERATION_STATUSES)[number];

export interface OperationOutcome {
  /** Index into `ActionPlan.operations`. */
  operationIndex: number;
  status: OperationStatus;
  /** Epoch milliseconds; present for `succeeded` and `failed`. */
  completedAt?: number;
  /** Present exactly when `status` is `succeeded`. */
  historyEntryId?: string;
  /** Present exactly when `status` is `failed`. */
  error?: FolioErrorPayload;
}

export const BATCH_STOP_REASONS = ["completed", "failed", "cancelled"] as const;

export type BatchStopReason = (typeof BATCH_STOP_REASONS)[number];

/** What actually happened to an approved plan. One outcome per operation. */
export interface BatchResult {
  planId: string;
  planDigest: ContentHash;
  startedAt: number;
  finishedAt: number;
  outcomes: OperationOutcome[];
  stopReason: BatchStopReason;
}

export interface HistoryEntry {
  id: string;
  planId: string;
  operationIndex: number;
  appliedAt: number;
  documentId?: DocumentId;
  /** Absent for `create`. */
  beforeRelativePath?: RelativePath;
  /** Absent when the operation removed a path. */
  afterRelativePath?: RelativePath;
  /** Absent for `create`. */
  beforeContentHash?: ContentHash;
  /** The state Undo expects to find before reversing this entry. */
  afterContentHash: ContentHash;
  /** False when the previous content could not be retained; Undo is then refused. */
  recoverable: boolean;
  undoneAt?: number;
}

export type UndoConflictReason =
  "externallyModified" | "missing" | "destinationOccupied" | "notRecoverable";

export interface UndoConflict {
  historyEntryId: string;
  documentId?: DocumentId;
  relativePath: RelativePath;
  expectedContentHash?: ContentHash;
  /** Null when the file is gone. */
  observedContentHash: ContentHash | null;
  reason: UndoConflictReason;
}

/**
 * Undo is whole-batch: if any entry conflicts, no file changes and the blocking
 * file is named. Newer external edits are preserved.
 */
export interface UndoPreflight {
  planId: string;
  entryIds: string[];
  conflicts: UndoConflict[];
  undoable: boolean;
}

/* ----------------------------------------------------------------- restart */

/**
 * After a restart Folio restores explicitly selected folders and unfinished
 * previews, revalidates folder access, and drops every approval. A restored
 * preview always requires fresh approval, and mutations never resume
 * automatically.
 */
export interface RestoredPreview {
  plan: ActionPlan;
  workspaceAvailable: boolean;
  requiresFreshApproval: true;
}

export interface RestoredSession {
  workspaces: WorkspaceInfo[];
  previews: RestoredPreview[];
}

/* --------------------------------------------------------------- model lab */

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
  /** Null when the task was not graded. Never a self-graded aggregate. */
  correctness: boolean | null;
  /** Peak RAM of the measured process, not the whole device. Null when unavailable. */
  peakProcessRamBytes: number | null;
  modelDiskBytes: number;
}

export type BenchmarkMemoryProcess = "llama-server" | "folio";

/** One measured process. `peakBytes` is null, with a reason, when unavailable. */
export interface BenchmarkMemory {
  process: BenchmarkMemoryProcess;
  pid: number | null;
  peakBytes: number | null;
  /** The span the peak covers, e.g. process lifetime since its start. */
  scope: string;
  /** How the peak was read, e.g. `PeakWorkingSetSize`. */
  method: string;
  unavailableReason?: string;
}

export interface BenchmarkCheck {
  name: string;
  /** Null when the check could not be evaluated; never a guessed pass. */
  passed: boolean | null;
  detail: string;
}

/** A person's judgment of one recorded output. Appended, never overwritten. */
export interface BenchmarkReview {
  status: "correct" | "incorrect" | "partiallyCorrect";
  reviewer: string;
  reviewedAt: number;
  notes?: string;
  /** The `outputSha256` of the output the reviewer actually read. */
  outputSha256: string;
}

/**
 * Issue #8's additive Model Lab record. It extends the frozen
 * `BenchmarkResult` and stays assignable to it. There is no aggregate or
 * self-graded score anywhere: `correctness` comes only from deterministic
 * checks against the labelled suite, and summary records keep it `null`.
 * `reviews: []` means "Not reviewed".
 */
export interface BenchmarkRecord extends BenchmarkResult {
  id: string;
  runId: string;
  /** Unix milliseconds. */
  createdAt: number;
  /** `frozen` stays false until a held-out suite is frozen. */
  suite: { id: string; sha256: string; frozen: boolean };
  /** Hash of the prompt templates the run used. */
  promptSha256: string;
  model: {
    id: string;
    role: ModelRole;
    repo: string;
    revision: string;
    quantization: string;
    files: { path: string; sha256: string; bytes: number }[];
  };
  /** Retrieval rows: the same as `model.id`. */
  embeddingModelId: string;
  runtimeDetail: { name: "llama.cpp" | "onnxruntime"; version: string };
  /** `installedRamBytes` is installed capacity, never usage. */
  host: {
    os: string;
    osVersion: string | null;
    arch: string;
    cpuBrand: string | null;
    logicalCpus: number;
    installedRamBytes: number | null;
  };
  conditions: {
    nCtx: number;
    maxOutputTokens: number;
    maxPassages: number;
    temperature: number;
    seed: number;
    threads: number;
    corpusSha256: string;
    /** The operating system's file cache is never controlled. */
    pageCache: "notControlled";
  };
  /**
   * `cold` is true only for the first request after the process restarted.
   * Startup time is separate from `taskDurationMs`. A task with several
   * requests is cold only through its first one.
   */
  timing: {
    processStartMs: number | null;
    requestsInTask: number;
    requestsSinceProcessStart: number;
    requestPosition: "firstRequestAfterServerRestart" | "immediateRepeat";
  };
  /**
   * The llama-server values actually used, not the defaults assumed. Null for
   * retrieval rows, which run in-process with no server.
   */
  serverSettings: {
    startupWarmup: "default-on" | "disabled";
    cachePrompt: boolean;
  } | null;
  observation: "single cold/repeat pair; initial observation, not a stable performance estimate";
  memory: BenchmarkMemory[];
  /** Equals `modelDiskBytes`: the model's own files, never installed size. */
  modelFileBytes: number;
  objectiveChecks: BenchmarkCheck[];
  /** The full raw outcome, kept so a reviewer can read what was produced. */
  output: unknown;
  outputSha256: string;
  reviews: BenchmarkReview[];
  schemaVersion: 1;
  apply: { status: "notRun"; reason: string };
}

export type BenchmarkRunStatus =
  "running" | "completed" | "cancelled" | "failed";

/** The summary row of one Model Lab run. */
export interface BenchmarkRunSummary {
  runId: string;
  status: BenchmarkRunStatus;
  /** The embedding model, then the generation models in the order run. */
  requestedModelIds: string[];
  suite: BenchmarkRecord["suite"];
  corpusSha256: string;
  host: BenchmarkRecord["host"];
  serverSettings: NonNullable<BenchmarkRecord["serverSettings"]>;
  startedAt: number;
  endedAt: number | null;
  /** Building the passage index is not a case; its time is kept here. */
  indexBuildMs: number | null;
  error?: string;
  schemaVersion: 1;
}

/**
 * A manifest model as Model Lab sees it. `modelFileBytes` is the pinned size
 * of the model's own files, never an installed size. `runnable` means
 * installed and hash-verified.
 */
export interface LabModel {
  id: string;
  role: ModelRole;
  repo: string;
  revision: string;
  quantization: string;
  modelFileBytes: number;
  status: ModelInstallStatus;
  runnable: boolean;
  selected: boolean;
}

export interface LabRunRequest {
  embeddingModelId: string;
  /** Run one at a time, in this order. Nothing is substituted. */
  generationModelIds: string[];
}

/** `folio://lab-progress`. `finished`, `cancelled` and `failed` end a run. */
export interface LabProgress {
  runId: string;
  step: string;
  caseId: string | null;
  modelId: string | null;
  error?: string | null;
}

/* -------------------------------------------------------- persistent index */

/**
 * `indexed`: current content is searchable. `unsupported`: readable but not
 * indexable (e.g. a scanned PDF without a text layer). `failed`: never indexed
 * successfully. `stale`: the file changed but could not be re-read; search
 * shows the previous version, whose hash is `contentHash`.
 */
export type IndexStatus = "indexed" | "unsupported" | "failed" | "stale";

/** A document as recorded by the native index. */
export interface IndexedDocument extends DocumentRecord {
  contentHash: ContentHash;
  status: IndexStatus;
  statusMessage?: string;
  indexedAtMs?: number;
  /**
   * For a `stale` or `failed` document: Local Sync will not read it again
   * before this time unless the file changes or the user asks to check again.
   */
  retryAfterMs?: number;
}

/** A folder chosen in an earlier session; restoring it revalidates access. */
export interface KnownWorkspace {
  id: WorkspaceId;
  rootPath: string;
  authorizedAt: number;
  lastOpenedAt: number | null;
  /** False when the folder is gone or no longer readable. */
  available: boolean;
}

export interface IndexProgress {
  workspaceId: WorkspaceId;
  phase: "discovering" | "indexing" | "linking" | "done" | "cancelled";
  processed: number;
  total: number;
  currentPath?: RelativePath;
}

/** Counts describe one scan; `unchanged` documents were not re-extracted. */
export interface ScanSummary {
  workspaceId: WorkspaceId;
  total: number;
  added: number;
  updated: number;
  unchanged: number;
  removed: number;
  unsupported: number;
  failed: number;
  stale: number;
  /**
   * `failed` and `stale` documents not read again this scan because they
   * failed the same way recently. Already counted in `failed` or `stale`.
   */
  deferred: number;
  /** Entries that could not be read or identified. */
  skipped: number;
  cancelled: boolean;
  durationMs: number;
}

/** Documents whose bytes were re-read and found identical; not a similarity judgement. */
export interface DuplicateGroup {
  contentHash: ContentHash;
  sizeBytes: number;
  documents: IndexedDocument[];
}

export type ExplicitReference = Extract<
  Relationship,
  { type: "explicitReference" }
>;

/**
 * A chunk without a vector in the given embedding space. Echo `contentHash`
 * when storing its vector: chunk ids can be reused after a rescan, and a vector
 * for text the chunk no longer holds is refused (`evidenceInvalid`,
 * `details.reason` = `chunkChanged`).
 */
export interface PendingChunk {
  chunkId: number;
  documentId: DocumentId;
  text: string;
  contentHash: ContentHash;
}

/** Exact cosine match within a single embedding space. */
export interface VectorCandidate {
  chunkId: number;
  score: number;
  spaceFingerprint: EmbeddingSpaceFingerprint;
  passage: SourcePassage;
}

/* ------------------------------------------------------------ native writer */

/**
 * What `apply_plan` reports once any operation has run: the durable batch record, whether
 * the plan's record was stored after the files changed, and whether the index caught up.
 * `historySettled: false` does not undo the outcomes in `batch`; those files did change.
 */
export interface ApplyReport {
  batch: BatchResult;
  historySettled: boolean;
  indexRefreshed: boolean;
}

/** An Undo that stops partway leaves `remainingEntryIds` pending; preview again to finish. */
export interface UndoReport {
  planId: string;
  undoneEntryIds: string[];
  remainingEntryIds: string[];
  error?: FolioErrorPayload;
  indexRefreshed: boolean;
}

/** An Organization Suggestion for a filename; `operation` still needs a preview and approval. */
export interface OrganizationSuggestion {
  documentId: DocumentId;
  relativePath: RelativePath;
  suggestedRelativePath: RelativePath;
  reason: string;
  operation: FileOperation;
}

/** Duplicate groups are evidence only: nothing is moved or deleted because of them. */
export interface OrganizationSuggestions {
  duplicateGroups: DuplicateGroup[];
  filenames: OrganizationSuggestion[];
}

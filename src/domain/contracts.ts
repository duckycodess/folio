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

export interface SourcePassage {
  documentId: DocumentId;
  /** UTF-16 code-unit offsets into the extracted document text. */
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
  embeddingSpaceId?: string;
}

export type ProviderErrorCode =
  | "modelNotInstalled"
  | "modelCorrupt"
  | "runtimeMissing"
  | "runtimeStartFailed"
  | "generationBusy"
  | "cancelled"
  | "contextLimit"
  | "invalidModelOutput"
  | "embeddingSpaceMismatch"
  | "noEvidence"
  | "ioError";

export interface NativeProviderError {
  code: ProviderErrorCode;
  message: string;
  detail?: string;
}

export type ModelRole = "embedding" | "generation";

export interface ModelFile {
  path: string;
  sha256: string;
  bytes: number;
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
  | "notInstalled"
  | "downloading"
  | "verifying"
  | "installed"
  | "corrupt";

export interface ModelInstallState {
  id: string;
  status: ModelInstallStatus;
  modelFileBytes?: number;
  error?: NativeProviderError;
}

export type GroundedAnswerKind =
  | "fileSummary"
  | "partialSummary"
  | "answer"
  | "insufficientEvidence";

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
  ranges: CoverageRange[];
  complete: boolean;
}

export interface GroundedAnswer {
  text: string;
  sources: SourcePassage[];
  kind: GroundedAnswerKind;
  sentences: GroundedSentence[];
  coverage: CoverageEntry[];
  uncitedSentenceCount: number;
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

export interface ImpactCandidate {
  documentId: DocumentId;
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

export type OperationProposal =
  | {
      kind: "edit";
      documentId: DocumentId;
      find: string;
      replace: string;
      targetEvidence: SourcePassage;
    }
  | {
      kind: "rename" | "move";
      documentId: DocumentId;
      destinationRelativePath: string;
    }
  | {
      kind: "create";
      destinationRelativePath: string;
      content: string;
    };

export type InterpretationResult =
  | {
      status: "proposal";
      proposal: OperationProposal;
      requestLanguage: Language;
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

export interface WorkspaceInfo {
  id: string;
  rootPath: string;
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

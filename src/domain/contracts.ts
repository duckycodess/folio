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

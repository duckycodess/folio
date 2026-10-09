import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  AiRefreshProgress,
  AiRelationshipCoverage,
  AiRelationshipRefresh,
  LocalAiRefresh,
  DocumentId,
  EmbeddingSpaceFingerprint,
  GroundedResult,
  ProviderIndexStatus,
  InterpretationResult,
  SearchResult,
} from "../domain/contracts";
import { folioError, toFolioError } from "../domain/errors";

export function isAvailable(): boolean {
  return isTauri();
}

async function call<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!isAvailable()) {
    throw folioError(
      "modelNotInstalled",
      "Local AI adapters are available in the Folio desktop app.",
      { component: "runtime", reason: "browserPreview" },
    );
  }
  try {
    return await invoke<T>(command, args);
  } catch (cause) {
    throw toFolioError(cause);
  }
}

export function rebuildIndex(
  workspaceId: string,
): Promise<ProviderIndexStatus> {
  return call("rebuild_index", { workspaceId });
}

export function indexStatus(): Promise<ProviderIndexStatus> {
  return call("index_status");
}

export function semanticSearch(
  workspaceId: string,
  query: string,
  limit = 10,
): Promise<SearchResult[]> {
  return call("semantic_search", { workspaceId, query, limit });
}

export function summarizeDocument(
  workspaceId: string,
  documentId: string,
): Promise<GroundedResult> {
  return call("summarize_document", { workspaceId, documentId });
}

export function answerQuestion(
  workspaceId: string,
  question: string,
  documentId?: string,
): Promise<GroundedResult> {
  return call("answer_question", { workspaceId, question, documentId });
}

/** Runs discovery over vectors already persisted for this exact space. */
export function refreshAiConnections(
  workspaceId: string,
  spaceFingerprint?: EmbeddingSpaceFingerprint,
): Promise<AiRelationshipRefresh> {
  return call("refresh_ai_connections", { workspaceId, spaceFingerprint });
}

export function cancelAiConnections(): Promise<void> {
  return call("cancel_ai_connections");
}

/** Embedding sync, then progressive relationship discovery, with one Stop. */
export function refreshLocalAiIndex(
  workspaceId: string,
): Promise<LocalAiRefresh> {
  return call("refresh_local_ai_index", { workspaceId });
}

export function cancelLocalAiRefresh(): Promise<void> {
  return call("cancel_local_ai_refresh");
}

/** What Folio has compared for AI connections. Reads only; starts nothing. */
export function relationshipCoverage(
  workspaceId: string,
): Promise<AiRelationshipCoverage> {
  return call("relationship_coverage", { workspaceId });
}

export function onAiRefreshProgress(
  handler: (progress: AiRefreshProgress) => void,
): Promise<() => void> {
  return listen<AiRefreshProgress>("folio://ai-refresh-progress", (event) =>
    handler(event.payload),
  );
}

export function summarizeRelationships(
  workspaceId: string,
  documentIds: DocumentId[],
  focusDocumentId?: DocumentId,
  spaceFingerprint?: EmbeddingSpaceFingerprint,
): Promise<GroundedResult> {
  return call("summarize_relationships", {
    workspaceId,
    documentIds,
    focusDocumentId,
    spaceFingerprint,
  });
}

export function explainImpact(
  workspaceId: string,
  planId: string,
  documentId: DocumentId,
): Promise<GroundedResult> {
  return call("explain_impact", { workspaceId, planId, documentId });
}

export function interpretRequest(
  workspaceId: string,
  text: string,
): Promise<InterpretationResult> {
  return call("interpret_request", { workspaceId, text });
}

export function unloadGeneration(): Promise<void> {
  return call("unload_generation");
}

export function cancelGeneration(): Promise<void> {
  return call("cancel_generation");
}

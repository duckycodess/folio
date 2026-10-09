import { invoke, isTauri } from "@tauri-apps/api/core";
import type {
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

export function indexStatus(
  workspaceId?: string,
): Promise<ProviderIndexStatus> {
  return call("index_status", { workspaceId });
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

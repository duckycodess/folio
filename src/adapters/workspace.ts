import { invoke, isTauri } from "@tauri-apps/api/core";
import type { DocumentRecord, WorkspaceInfo } from "../domain/contracts";

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

interface NativeDocument {
  id: string;
  relativePath: string;
  name: string;
  sizeBytes: number;
}

export async function chooseWorkspace(): Promise<{
  info: WorkspaceInfo;
  documents: DocumentRecord[];
} | null> {
  const info = await invoke<WorkspaceInfo | null>("choose_workspace");
  if (!info) return null;
  const rows = await invoke<NativeDocument[]>("list_documents", {
    workspaceId: info.id,
  });
  return {
    info,
    documents: rows.map((row) => ({
      ...row,
      title: row.name,
      language: "unknown",
    })),
  };
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

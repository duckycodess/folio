import type { DocumentRecord, Language } from "../domain/contracts";

export function fileKind(document: DocumentRecord): string {
  switch (document.mediaType) {
    case "text/markdown":
      return "Markdown";
    case "application/pdf":
      return "PDF";
    default:
      return "Text";
  }
}

export function languageLabel(language: Language): string {
  switch (language) {
    case "en":
      return "English";
    case "fil":
      return "Filipino";
    case "mixed":
      return "Taglish";
    default:
      return "Not detected";
  }
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function formatModified(ms: number | undefined): string {
  if (ms === undefined) return "Unknown";
  return new Date(ms).toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}

/** The folder part of a workspace-relative path, or "Top folder". */
export function folderOf(relativePath: string): string {
  const parts = relativePath.split("/");
  return parts.length > 1 ? parts.slice(0, -1).join(" / ") : "Top folder";
}

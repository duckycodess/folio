import type {
  DocumentRecord,
  EmbeddingSpace,
  Relationship,
  SearchResult,
} from "./contracts";

function words(value: string): string[] {
  return (
    value
      .toLocaleLowerCase()
      .normalize("NFKC")
      .match(/[\p{L}\p{N}]+/gu) ?? []
  );
}

/** Development fallback; this is deliberately not semantic retrieval. */
export function keywordSearch(
  documents: DocumentRecord[],
  query: string,
): SearchResult[] {
  const terms = [...new Set(words(query))];
  return documents
    .map((document) => {
      const tokens = new Set(
        words(`${document.name} ${document.title} ${document.content ?? ""}`),
      );
      const score = terms.length
        ? terms.filter((term) => tokens.has(term)).length / terms.length
        : 1;
      const content = document.content ?? "";
      const offset = terms.length
        ? Math.max(
            0,
            content
              .toLocaleLowerCase()
              .indexOf(
                terms.find((term) =>
                  content.toLocaleLowerCase().includes(term),
                ) ?? "",
              ),
          )
        : 0;
      const start = Math.max(0, offset - 40);
      const end = Math.min(content.length, start + 200);
      return {
        document,
        score,
        method: "keyword" as const,
        passages: content
          ? [
              {
                documentId: document.id,
                start,
                end,
                text: content.slice(start, end),
              },
            ]
          : [],
      };
    })
    .filter((result) => result.score > 0)
    .sort(
      (a, b) =>
        b.score - a.score || a.document.name.localeCompare(b.document.name),
    );
}

function linkedPath(sourcePath: string, link: string): string | undefined {
  if (/^(?:[a-z]+:|\/|\\)/i.test(link)) return undefined;
  let decoded: string;
  try {
    decoded = decodeURIComponent(link.split(/[?#]/)[0]);
  } catch {
    return undefined;
  }
  if (!decoded || /^(?:[a-z]+:|\/|\\)/i.test(decoded)) return undefined;
  const parts = sourcePath.split("/").slice(0, -1);
  for (const part of decoded.split("/")) {
    if (!part || part === ".") continue;
    if (part === "..") {
      if (!parts.length) return undefined;
      parts.pop();
    } else parts.push(part);
  }
  return parts.join("/");
}

/** Explicit Markdown links are evidence; no similarity or dependency is inferred. */
export function discoverExplicitReferences(
  documents: DocumentRecord[],
): Relationship[] {
  const byPath = new Map(
    documents.map((document) => [document.relativePath, document]),
  );
  const relationships: Relationship[] = [];
  for (const document of documents) {
    const content = document.content ?? "";
    for (const match of content.matchAll(/\[[^\]]+\]\(([^)]+)\)/g)) {
      const path = linkedPath(document.relativePath, match[1]);
      const target = path ? byPath.get(path) : undefined;
      if (!target || target.id === document.id) continue;
      relationships.push({
        sourceId: document.id,
        targetId: target.id,
        type: "explicitReference",
        provenance: "documentLink",
        evidence: [
          {
            documentId: document.id,
            start: match.index!,
            end: match.index! + match[0].length,
            text: match[0],
          },
        ],
      });
    }
  }
  return relationships;
}

export function sameEmbeddingSpace(
  a: EmbeddingSpace,
  b: EmbeddingSpace,
): boolean {
  return (
    a.modelId === b.modelId &&
    a.revision === b.revision &&
    a.quantization === b.quantization &&
    a.dimensions === b.dimensions &&
    a.preprocessingFingerprint === b.preprocessingFingerprint
  );
}

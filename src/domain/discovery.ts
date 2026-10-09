import type {
  DocumentRecord,
  Relationship,
  SearchResult,
  SourcePassage,
} from "./contracts";
import { normalizeRelativePath } from "./identity";
import { passageFromUtf16Range } from "./offsets";

const PASSAGE_LEAD = 40;
const PASSAGE_LENGTH = 200;

function words(value: string): string[] {
  return (
    value
      .toLocaleLowerCase()
      .normalize("NFKC")
      .match(/[\p{L}\p{N}]+/gu) ?? []
  );
}

/**
 * Development fallback. This is keyword filtering, not semantic retrieval, and
 * `method` says so. Passages are produced only for documents whose text and
 * revision are both known, so evidence is never attached to an unknown
 * revision.
 */
export function keywordSearch(
  documents: DocumentRecord[],
  query: string,
): SearchResult[] {
  const terms = [...new Set(words(query))];
  return documents
    .map((document) => {
      const content = document.content ?? "";
      const tokens = new Set(
        words(`${document.name} ${document.title} ${content}`),
      );
      const score = terms.length
        ? terms.filter((term) => tokens.has(term)).length / terms.length
        : 1;
      const lowered = content.toLocaleLowerCase();
      const hit = terms.find((term) => lowered.includes(term));
      const offset = hit ? Math.max(0, lowered.indexOf(hit)) : 0;
      const startIndex = Math.max(0, offset - PASSAGE_LEAD);
      const endIndex = Math.min(content.length, startIndex + PASSAGE_LENGTH);
      const passages: SourcePassage[] =
        content && document.contentHash
          ? [
              passageFromUtf16Range({
                documentId: document.id,
                documentContentHash: document.contentHash,
                text: content,
                startIndex,
                endIndex,
              }),
            ]
          : [];
      return { document, score, method: "keyword" as const, passages };
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
  try {
    return normalizeRelativePath(parts.join("/"));
  } catch {
    return undefined;
  }
}

/**
 * Explicit Markdown links are evidence of a reference. No similarity and no
 * dependency is inferred, and a link that leaves the workspace is not a
 * connection.
 */
export function discoverExplicitReferences(
  documents: DocumentRecord[],
): Relationship[] {
  const byPath = new Map(
    documents.map((document) => [document.relativePath, document]),
  );
  const relationships: Relationship[] = [];
  for (const document of documents) {
    const content = document.content;
    if (!content || !document.contentHash) continue;
    for (const match of content.matchAll(/\[[^\]]+\]\(([^)]+)\)/g)) {
      const path = linkedPath(document.relativePath, match[1]);
      const target = path ? byPath.get(path) : undefined;
      if (!target || !target.contentHash || target.id === document.id) continue;
      const startIndex = match.index ?? 0;
      relationships.push({
        type: "explicitReference",
        provenance: "documentLink",
        sourceId: document.id,
        targetId: target.id,
        sourceContentHash: document.contentHash,
        targetContentHash: target.contentHash,
        link: {
          rawTarget: match[1],
          resolvedRelativePath: target.relativePath,
        },
        evidence: [
          passageFromUtf16Range({
            documentId: document.id,
            documentContentHash: document.contentHash,
            text: content,
            startIndex,
            endIndex: startIndex + match[0].length,
          }),
        ],
      });
    }
  }
  return relationships;
}

export {
  assertSameEmbeddingSpace,
  embeddingSpaceFingerprint,
  sameEmbeddingSpace,
} from "./identity";

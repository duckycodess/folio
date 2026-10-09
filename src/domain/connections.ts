import type {
  DocumentId,
  DocumentRecord,
  DuplicateGroup,
  Relationship,
  RelationshipProvenance,
  SourcePassage,
} from "./contracts";
import { sliceByUtf8Offsets, utf8OffsetToUtf16Index } from "./offsets";
import { relationshipEvidence } from "./relationships";

export type ConnectionKind =
  "exactDuplicate" | "explicitReference" | "sharedFactCandidate" | "similarity";

/** How the connection was established; "contentHash" means identical bytes. */
export type ConnectionProvenance = RelationshipProvenance | "contentHash";

/**
 * One related document as seen from a given document. Every relationship of
 * the same kind with the same document is folded into one connection, so the
 * list never repeats a file and never drops one.
 */
export interface Connection {
  kind: ConnectionKind;
  otherId: DocumentId;
  /** Links have a direction; the other kinds are `mutual`. */
  direction: "outgoing" | "incoming" | "mutual";
  provenance: ConnectionProvenance;
  /** Each passage is located in the document its `documentId` names. */
  evidence: SourcePassage[];
  /** Similarity in [0, 1]; the highest of the folded relationships. */
  score?: number;
  confidence?: number;
}

const KIND_ORDER: ConnectionKind[] = [
  "exactDuplicate",
  "explicitReference",
  "sharedFactCandidate",
  "similarity",
];

function relationshipKey(relationship: Relationship): string {
  const first = relationshipEvidence(relationship)[0];
  return [
    relationship.type,
    relationship.sourceId,
    relationship.targetId,
    first ? `${first.start}:${first.end}` : "",
  ].join("|");
}

/**
 * Combines relationships from several sources (the persistent index and links
 * read from opened files) without listing the same evidence twice.
 */
export function mergeRelationships(
  ...sources: Relationship[][]
): Relationship[] {
  const seen = new Map<string, Relationship>();
  for (const source of sources)
    for (const relationship of source) {
      const key = relationshipKey(relationship);
      if (!seen.has(key)) seen.set(key, relationship);
    }
  return [...seen.values()];
}

function passageKey(passage: SourcePassage): string {
  return `${passage.documentId}|${passage.start}|${passage.end}`;
}

/** Every connection of one document, ordered by kind; nothing is truncated. */
export function connectionsFor(
  documentId: DocumentId,
  relationships: Relationship[],
  duplicates: DuplicateGroup[] = [],
): Connection[] {
  const byKey = new Map<string, Connection>();

  function add(next: Connection) {
    const key = `${next.kind}|${next.otherId}`;
    const current = byKey.get(key);
    if (!current) {
      byKey.set(key, { ...next, evidence: [...next.evidence] });
      return;
    }
    if (current.direction !== next.direction) current.direction = "mutual";
    const known = new Set(current.evidence.map(passageKey));
    for (const passage of next.evidence)
      if (!known.has(passageKey(passage))) current.evidence.push(passage);
    if (next.score !== undefined)
      current.score = Math.max(current.score ?? 0, next.score);
    if (next.confidence !== undefined)
      current.confidence = Math.max(current.confidence ?? 0, next.confidence);
  }

  for (const relationship of relationships) {
    const outgoing = relationship.sourceId === documentId;
    if (!outgoing && relationship.targetId !== documentId) continue;
    const otherId = outgoing ? relationship.targetId : relationship.sourceId;
    if (otherId === documentId) continue;
    switch (relationship.type) {
      case "explicitReference":
        add({
          kind: "explicitReference",
          otherId,
          direction: outgoing ? "outgoing" : "incoming",
          provenance: relationship.provenance,
          evidence: relationshipEvidence(relationship),
        });
        break;
      case "similarity":
        add({
          kind: "similarity",
          otherId,
          direction: "mutual",
          provenance: relationship.provenance,
          evidence: relationshipEvidence(relationship),
          score: relationship.score,
        });
        break;
      case "sharedFactCandidate":
        add({
          kind: "sharedFactCandidate",
          otherId,
          direction: "mutual",
          provenance: relationship.provenance,
          evidence: relationshipEvidence(relationship),
          confidence: relationship.confidence,
        });
        break;
    }
  }

  for (const group of duplicates) {
    if (!group.documents.some((document) => document.id === documentId))
      continue;
    for (const document of group.documents)
      if (document.id !== documentId)
        add({
          kind: "exactDuplicate",
          otherId: document.id,
          direction: "mutual",
          provenance: "contentHash",
          evidence: [],
        });
  }

  return [...byKey.values()].sort(
    (a, b) => KIND_ORDER.indexOf(a.kind) - KIND_ORDER.indexOf(b.kind),
  );
}

/**
 * Exact duplicates among documents whose bytes Folio has hashed. Equal hashes
 * mean identical contents; nothing about similarity is inferred.
 */
export function localDuplicates(documents: DocumentRecord[]): DuplicateGroup[] {
  const byHash = new Map<string, DocumentRecord[]>();
  for (const document of documents) {
    if (!document.contentHash) continue;
    const group = byHash.get(document.contentHash) ?? [];
    group.push(document);
    byHash.set(document.contentHash, group);
  }
  return [...byHash.entries()]
    .filter(([, group]) => group.length > 1)
    .map(([contentHash, group]) => ({
      contentHash,
      sizeBytes: group[0].sizeBytes,
      documents: group.map((document) => ({
        ...document,
        contentHash,
        status: "indexed" as const,
      })),
    }));
}

export interface ConnectionLabel {
  /** Short type label for a badge. */
  type: string;
  /** How Folio knows, in plain words. */
  provenance: string;
}

/**
 * Plain-language labels. Links and identical bytes are facts read from the
 * files and are never described as AI output; only model provenance says so.
 */
export function describeConnection(connection: Connection): ConnectionLabel {
  switch (connection.kind) {
    case "exactDuplicate":
      return {
        type: "Exact duplicate",
        provenance: "Same contents, byte for byte",
      };
    case "explicitReference":
      return {
        type:
          connection.direction === "outgoing"
            ? "This file links to it"
            : connection.direction === "incoming"
              ? "Links to this file"
              : "Linked both ways",
        provenance: "Link written in the file",
      };
    case "similarity":
      return {
        type: "Similar content",
        provenance:
          connection.score === undefined
            ? "Found by comparing passages"
            : `Found by comparing passages (${Math.round(connection.score * 100)}% similar)`,
      };
    case "sharedFactCandidate":
      return {
        type: "May state the same fact",
        provenance:
          connection.provenance === "model"
            ? "Suggested by the local AI model; check before relying on it"
            : "Found by comparing passages; check before relying on it",
      };
  }
}

export type PassageState = "current" | "changed" | "unread";

/** Whether a passage still points at the text the document has now. */
export function passageState(
  passage: SourcePassage,
  document: Pick<DocumentRecord, "content" | "contentHash">,
): PassageState {
  if (document.content === undefined || !document.contentHash) return "unread";
  if (document.contentHash !== passage.documentContentHash) return "changed";
  try {
    return sliceByUtf8Offsets(document.content, passage.start, passage.end) ===
      passage.text
      ? "current"
      : "changed";
  } catch {
    return "changed";
  }
}

/** The passage as string indices into `content`, or `null` when it is stale. */
export function highlightRange(
  passage: SourcePassage,
  document: Pick<DocumentRecord, "content" | "contentHash">,
): [number, number] | null {
  if (passageState(passage, document) !== "current" || !document.content)
    return null;
  return [
    utf8OffsetToUtf16Index(document.content, passage.start),
    utf8OffsetToUtf16Index(document.content, passage.end),
  ];
}

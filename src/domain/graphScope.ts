import type { Connection } from "./connections";
import type { DocumentId, DocumentRecord } from "./contracts";
import { hasSearchWords, keywordSearch } from "./discovery";

/** Where a Graph view starts: everything, one file, one folder or a topic. */
export type GraphStart =
  | { kind: "all" }
  | { kind: "file"; documentId: DocumentId }
  /** A folder inside the open folder, with everything below it. */
  | { kind: "folder"; folder: string }
  /** A keyword topic, matched against file names and text Folio has read. */
  | { kind: "topic"; term: string };

export interface GraphPair {
  from: DocumentRecord;
  to: DocumentRecord;
  connection: Connection;
}

export const RELATIONSHIP_SUMMARY_SCOPE_CAP = 50;

export interface RelationshipSummaryScope {
  documentIds: DocumentId[];
  totalDocuments: number;
}

function connectionStrength(connection: Connection): number {
  if (connection.score !== undefined) return connection.score;
  if (connection.confidence !== undefined) return connection.confidence;
  return connection.kind === "explicitReference" ||
    connection.kind === "exactDuplicate"
    ? 1
    : 0;
}

/**
 * Chooses a bounded summary scope without changing the graph shown to the
 * reader. A selected file comes first, followed by its strongest neighbours;
 * the remaining strongest pairs fill the scope deterministically.
 */
export function relationshipSummaryScope(
  pairs: GraphPair[],
  focusDocumentId?: DocumentId,
  max = RELATIONSHIP_SUMMARY_SCOPE_CAP,
): RelationshipSummaryScope {
  const all = new Set<DocumentId>();
  for (const pair of pairs) {
    all.add(pair.from.id);
    all.add(pair.to.id);
  }
  const limit = Math.max(0, max);
  const ids: DocumentId[] = [];
  const add = (id: DocumentId) => {
    if (ids.length < limit && !ids.includes(id)) ids.push(id);
  };
  const ranked = pairs
    .map((pair, index) => ({ pair, index }))
    .sort(
      (left, right) =>
        connectionStrength(right.pair.connection) -
          connectionStrength(left.pair.connection) ||
        left.index - right.index,
    );
  if (focusDocumentId && all.has(focusDocumentId)) add(focusDocumentId);
  for (const { pair } of ranked) {
    if (
      focusDocumentId &&
      (pair.from.id === focusDocumentId || pair.to.id === focusDocumentId)
    ) {
      add(pair.from.id);
      add(pair.to.id);
    }
  }
  for (const { pair } of ranked) {
    add(pair.from.id);
    add(pair.to.id);
  }
  return { documentIds: ids, totalDocuments: all.size };
}

function inFolder(document: DocumentRecord, folder: string): boolean {
  return !folder || document.relativePath.startsWith(`${folder}/`);
}

/** The files a start covers; connections must touch one of them. */
export function startingFiles(
  documents: DocumentRecord[],
  start: GraphStart,
): Set<DocumentId> {
  switch (start.kind) {
    case "all":
      return new Set(documents.map((document) => document.id));
    case "file":
      return new Set(
        documents.some((document) => document.id === start.documentId)
          ? [start.documentId]
          : [],
      );
    case "folder":
      return new Set(
        documents
          .filter((document) => inFolder(document, start.folder))
          .map((document) => document.id),
      );
    case "topic":
      return new Set(
        hasSearchWords(start.term)
          ? keywordSearch(documents, start.term).map(
              (result) => result.document.id,
            )
          : [],
      );
  }
}

/**
 * Each connection that touches the start, once. From one file, that file is
 * always the first end; otherwise the end that comes first by path is.
 */
export function graphPairs(
  documents: DocumentRecord[],
  connectionsOf: (documentId: DocumentId) => Connection[],
  start: GraphStart,
): GraphPair[] {
  const sorted = [...documents].sort((a, b) =>
    a.relativePath.localeCompare(b.relativePath),
  );
  const byId = new Map(sorted.map((document) => [document.id, document]));
  if (start.kind === "file") {
    const from = byId.get(start.documentId);
    if (!from) return [];
    return connectionsOf(from.id).flatMap((connection) => {
      const to = byId.get(connection.otherId);
      return to ? [{ from, to, connection }] : [];
    });
  }
  const covered = startingFiles(sorted, start);
  const order = new Map(sorted.map((document, index) => [document.id, index]));
  const pairs: GraphPair[] = [];
  for (const from of sorted)
    for (const connection of connectionsOf(from.id)) {
      const to = byId.get(connection.otherId);
      if (!to || order.get(from.id)! >= order.get(to.id)!) continue;
      if (covered.has(from.id) || covered.has(to.id))
        pairs.push({ from, to, connection });
    }
  return pairs;
}

/**
 * Confirmed connections are facts read from the files: a link written in one,
 * or identical bytes. Everything else is a suggestion to check.
 */
export function isConfirmed(connection: Connection): boolean {
  return (
    connection.kind === "explicitReference" ||
    connection.kind === "exactDuplicate"
  );
}

export interface FolderCount {
  /** "" is the top of the open folder. */
  folder: string;
  files: number;
}

/**
 * How many distinct files in the connections sit in each folder, most first.
 * A count, not a written summary: it needs no model.
 */
export function folderSpread(pairs: GraphPair[]): FolderCount[] {
  const files = new Map<DocumentId, DocumentRecord>();
  for (const { from, to } of pairs) {
    files.set(from.id, from);
    files.set(to.id, to);
  }
  const counts = new Map<string, number>();
  for (const document of files.values()) {
    const folder = document.relativePath.split("/").slice(0, -1).join("/");
    counts.set(folder, (counts.get(folder) ?? 0) + 1);
  }
  return [...counts.entries()]
    .map(([folder, files]) => ({ folder, files }))
    .sort((a, b) => b.files - a.files || a.folder.localeCompare(b.folder));
}

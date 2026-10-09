import {
  connectionsFor,
  type Connection,
  type ConnectionKind,
  type ConnectionProvenance,
  type DuplicateSet,
} from "./connections";
import type {
  DocumentId,
  DocumentRecord,
  MediaType,
  Relationship,
} from "./contracts";

/**
 * Above this many files the map shows the selected file's neighbourhood
 * instead of every file. The list always shows every connection.
 */
export const MAX_MAP_NODES = 400;

/** Two files and one connection between them, as the list shows it. */
export interface GraphPair {
  from: DocumentRecord;
  to: DocumentRecord;
  connection: Connection;
}

/** Path order, the order of the list and of Page Up/Page Down in the map. */
export function byPath(
  a: Pick<DocumentRecord, "id" | "relativePath">,
  b: Pick<DocumentRecord, "id" | "relativePath">,
): number {
  return (
    a.relativePath.localeCompare(b.relativePath) ||
    (a.id < b.id ? -1 : a.id > b.id ? 1 : 0)
  );
}

/**
 * `connectionsFor` over only the relationships and duplicate groups that touch
 * each document, so a whole folder costs one pass instead of one per file.
 */
function connectionIndex(
  relationships: Relationship[],
  duplicates: DuplicateSet[],
): (id: DocumentId) => Connection[] {
  const relationshipsOf = new Map<DocumentId, Relationship[]>();
  const duplicatesOf = new Map<DocumentId, DuplicateSet[]>();
  function add<T>(map: Map<DocumentId, T[]>, id: DocumentId, item: T) {
    const list = map.get(id);
    if (list) {
      if (list[list.length - 1] !== item) list.push(item);
    } else map.set(id, [item]);
  }
  for (const relationship of relationships) {
    add(relationshipsOf, relationship.sourceId, relationship);
    add(relationshipsOf, relationship.targetId, relationship);
  }
  for (const group of duplicates)
    for (const document of group.documents)
      add(duplicatesOf, document.id, group);
  return (id) =>
    connectionsFor(id, relationshipsOf.get(id) ?? [], duplicatesOf.get(id));
}

/**
 * Each connection once, seen from the file that comes first by path. The list
 * and the map are both drawn from these pairs, so they never disagree.
 */
export function graphPairs(
  documents: DocumentRecord[],
  relationships: Relationship[],
  duplicates: DuplicateSet[],
): GraphPair[] {
  const sorted = [...documents].sort(byPath);
  const order = new Map(sorted.map((document, index) => [document.id, index]));
  const byId = new Map(sorted.map((document) => [document.id, document]));
  const connectionsOf = connectionIndex(relationships, duplicates);
  const pairs: GraphPair[] = [];
  for (const from of sorted)
    for (const connection of connectionsOf(from.id)) {
      const to = byId.get(connection.otherId);
      if (to && order.get(from.id)! < order.get(to.id)!)
        pairs.push({ from, to, connection });
    }
  return pairs;
}

/**
 * Links read from the files and identical bytes are facts; anything an
 * embedding or generative model produced is an inference to check.
 */
export type EdgeOrigin = "confirmed" | "inferred";

export function edgeOrigin(provenance: ConnectionProvenance): EdgeOrigin {
  switch (provenance) {
    case "documentLink":
    case "contentHash":
      return "confirmed";
    case "embedding":
    case "model":
      return "inferred";
  }
}

export interface GraphNode {
  id: DocumentId;
  name: string;
  relativePath: string;
  /** The folder inside the workspace; empty for the top folder. */
  folder: string;
  mediaType: MediaType;
  /** Connections drawn to this file, after any filter. */
  degree: number;
  kinds: Partial<Record<ConnectionKind, number>>;
}

export interface GraphEdge {
  id: string;
  /** For a one-way link, the file that contains it. */
  source: DocumentId;
  target: DocumentId;
  kind: ConnectionKind;
  /** `outgoing` is a link from `source` to `target`; the rest are `mutual`. */
  direction: "outgoing" | "mutual";
  provenance: ConnectionProvenance;
  origin: EdgeOrigin;
  evidenceCount: number;
}

export interface GraphModel {
  /** In path order. */
  nodes: GraphNode[];
  edges: GraphEdge[];
}

function folderOf(relativePath: string): string {
  return relativePath.split("/").slice(0, -1).join("/");
}

function withDegrees(nodes: GraphNode[], edges: GraphEdge[]): GraphNode[] {
  const kinds = new Map<DocumentId, Partial<Record<ConnectionKind, number>>>(
    nodes.map((node) => [node.id, {}]),
  );
  for (const edge of edges)
    for (const id of [edge.source, edge.target]) {
      const counts = kinds.get(id);
      if (counts) counts[edge.kind] = (counts[edge.kind] ?? 0) + 1;
    }
  return nodes.map((node) => {
    const counts = kinds.get(node.id)!;
    const degree = Object.values(counts).reduce((sum, n) => sum + n, 0);
    return { ...node, degree, kinds: counts };
  });
}

/** Every file is a node, connected or not; each pair is one edge per kind. */
export function buildGraph(
  documents: DocumentRecord[],
  pairs: GraphPair[],
): GraphModel {
  const nodes: GraphNode[] = [...documents].sort(byPath).map((document) => ({
    id: document.id,
    name: document.name,
    relativePath: document.relativePath,
    folder: folderOf(document.relativePath),
    mediaType: document.mediaType,
    degree: 0,
    kinds: {},
  }));
  const edges = pairs.map(({ from, to, connection }): GraphEdge => {
    const [source, target] =
      connection.direction === "incoming" ? [to.id, from.id] : [from.id, to.id];
    return {
      id: `${connection.kind}|${source}|${target}`,
      source,
      target,
      kind: connection.kind,
      direction: connection.direction === "mutual" ? "mutual" : "outgoing",
      provenance: connection.provenance,
      origin: edgeOrigin(connection.provenance),
      evidenceCount: connection.evidence.length,
    };
  });
  return { nodes: withDegrees(nodes, edges), edges };
}

/** The graph without the hidden kinds; files stay even if left unconnected. */
export function filterEdges(
  graph: GraphModel,
  hidden: ReadonlySet<ConnectionKind>,
): GraphModel {
  if (!hidden.size) return graph;
  const edges = graph.edges.filter((edge) => !hidden.has(edge.kind));
  return { nodes: withDegrees(graph.nodes, edges), edges };
}

/** Files within `depth` connections of `id`, and the edges among them. */
export function neighbourhood(
  graph: GraphModel,
  id: DocumentId,
  depth: number,
): GraphModel {
  const adjacent = new Map<DocumentId, DocumentId[]>();
  function link(from: DocumentId, to: DocumentId) {
    const list = adjacent.get(from);
    if (list) list.push(to);
    else adjacent.set(from, [to]);
  }
  for (const edge of graph.edges) {
    link(edge.source, edge.target);
    link(edge.target, edge.source);
  }
  const reached = new Set<DocumentId>();
  if (graph.nodes.some((node) => node.id === id)) reached.add(id);
  let frontier = [...reached];
  for (let step = 0; step < depth && frontier.length; step++) {
    const next: DocumentId[] = [];
    for (const current of frontier)
      for (const other of adjacent.get(current) ?? [])
        if (!reached.has(other)) {
          reached.add(other);
          next.push(other);
        }
    frontier = next;
  }
  const edges = graph.edges.filter(
    (edge) => reached.has(edge.source) && reached.has(edge.target),
  );
  return {
    nodes: withDegrees(
      graph.nodes.filter((node) => reached.has(node.id)),
      edges,
    ),
    edges,
  };
}

export interface MapSubset {
  graph: GraphModel;
  /** The file the map is centred on when it can't show every file. */
  centerId: DocumentId | null;
  /** Files left off the map; the list still has them. */
  omitted: number;
}

/**
 * What the map can show clearly. Small folders show everything. Large ones
 * show the files within two connections of `centerId` (or the most connected
 * file), then one, and at most `max` files, first by path.
 */
export function mapSubset(
  graph: GraphModel,
  centerId: DocumentId | null | undefined,
  max = MAX_MAP_NODES,
): MapSubset {
  if (graph.nodes.length <= max) return { graph, centerId: null, omitted: 0 };
  const center =
    graph.nodes.find((node) => node.id === centerId) ??
    graph.nodes.reduce((best, node) =>
      node.degree > best.degree ? node : best,
    );
  let subset = neighbourhood(graph, center.id, 2);
  if (subset.nodes.length > max) subset = neighbourhood(graph, center.id, 1);
  if (subset.nodes.length > max) {
    const kept = new Set([
      center.id,
      ...subset.nodes
        .filter((node) => node.id !== center.id)
        .slice(0, max - 1)
        .map((node) => node.id),
    ]);
    const edges = subset.edges.filter(
      (edge) => kept.has(edge.source) && kept.has(edge.target),
    );
    subset = {
      nodes: withDegrees(
        subset.nodes.filter((node) => kept.has(node.id)),
        edges,
      ),
      edges,
    };
  }
  return {
    graph: subset,
    centerId: center.id,
    omitted: graph.nodes.length - subset.nodes.length,
  };
}

const KIND_WORDS: Record<ConnectionKind, [string, string]> = {
  explicitReference: ["link", "links"],
  exactDuplicate: ["identical copy", "identical copies"],
  similarity: ["similar file found by AI", "similar files found by AI"],
  sharedFactCandidate: [
    "possible shared fact found by AI",
    "possible shared facts found by AI",
  ],
};

const SUMMARY_ORDER: ConnectionKind[] = [
  "explicitReference",
  "exactDuplicate",
  "similarity",
  "sharedFactCandidate",
];

/** "3 links, 1 identical copy"; only model-found kinds mention AI. */
export function describeKinds(
  kinds: Partial<Record<ConnectionKind, number>>,
): string {
  return SUMMARY_ORDER.filter((kind) => kinds[kind])
    .map((kind) => {
      const count = kinds[kind]!;
      return `${count} ${KIND_WORDS[kind][count === 1 ? 0 : 1]}`;
    })
    .join(", ");
}

/** The spoken name of a node: file, folder and what it connects to. */
export function describeNode(node: GraphNode): string {
  const folder = node.folder ? `in ${node.folder}` : "in the top folder";
  if (!node.degree) return `${node.name}, ${folder}, no connections shown`;
  const count = `${node.degree} ${node.degree === 1 ? "connection" : "connections"}`;
  return `${node.name}, ${folder}, ${count}: ${describeKinds(node.kinds)}`;
}

/** The short label drawn on an edge. Only inferred edges say "AI". */
export function edgeLabel(edge: GraphEdge): string {
  const base =
    edge.kind === "explicitReference"
      ? edge.direction === "mutual"
        ? "Links both ways"
        : "Link"
      : edge.kind === "exactDuplicate"
        ? "Identical copy"
        : edge.kind === "similarity"
          ? "Similar content"
          : "May state the same fact";
  return edge.origin === "inferred" ? `AI · ${base}` : base;
}

import { describe, expect, it } from "vitest";
import { loadFixtureDocuments } from "../adapters/workspace";
import { localDuplicates, type DuplicateSet } from "./connections";
import type { DocumentRecord, Relationship, SourcePassage } from "./contracts";
import { discoverExplicitReferences } from "./discovery";
import {
  buildGraph,
  describeNode,
  edgeLabel,
  edgeOrigin,
  filterEdges,
  graphPairs,
  mapSubset,
  neighbourhood,
  type GraphModel,
} from "./graph";
import { documentId } from "./test-support";

const HASH = "e".repeat(64);

function doc(relativePath: string): DocumentRecord {
  return {
    id: documentId(relativePath),
    relativePath,
    name: relativePath.split("/").at(-1)!,
    mediaType: "text/markdown",
  } as DocumentRecord;
}

function passage(documentId: string): SourcePassage {
  return {
    documentId,
    documentContentHash: HASH,
    start: 0,
    end: 4,
    offsetUnit: "utf8Byte",
    text: "text",
  };
}

function link(from: DocumentRecord, to: DocumentRecord): Relationship {
  return {
    type: "explicitReference",
    provenance: "documentLink",
    sourceId: from.id,
    targetId: to.id,
    sourceContentHash: HASH,
    targetContentHash: HASH,
    link: { rawTarget: to.name, resolvedRelativePath: to.relativePath },
    evidence: [passage(from.id)],
  };
}

function similar(a: DocumentRecord, b: DocumentRecord): Relationship {
  return {
    type: "similarity",
    provenance: "embedding",
    spaceFingerprint: "space",
    score: 0.8,
    sourceId: a.id,
    targetId: b.id,
    sourceContentHash: HASH,
    targetContentHash: HASH,
    sourceEvidence: [passage(a.id)],
    targetEvidence: [passage(b.id)],
  };
}

function copies(...documents: DocumentRecord[]): DuplicateSet {
  return { documents: documents.map(({ id }) => ({ id })) };
}

const PLAN = doc("projects/plan.md");
const NOTES = doc("meetings/notes.md");
const COPY = doc("archive/plan-copy.md");
const LONELY = doc("personal/grocery.md");
const FAR = doc("research/far.md");
const DOCUMENTS = [PLAN, NOTES, COPY, LONELY, FAR];

function graphOf(
  relationships: Relationship[],
  duplicates: DuplicateSet[] = [],
  documents = DOCUMENTS,
): GraphModel {
  return buildGraph(
    documents,
    graphPairs(documents, relationships, duplicates),
  );
}

describe("the graph drawn from the list's pairs", () => {
  it("has one edge per pair and kind, with links in both directions folded", () => {
    const graph = graphOf(
      [link(NOTES, PLAN), link(PLAN, NOTES), similar(NOTES, PLAN)],
      [copies(PLAN, COPY)],
    );
    const keys = graph.edges.map((edge) => `${edge.kind} ${edge.direction}`);
    expect(keys.sort()).toEqual([
      "exactDuplicate mutual",
      "explicitReference mutual",
      "similarity mutual",
    ]);
    const linkEdge = graph.edges.find((e) => e.kind === "explicitReference");
    expect(linkEdge?.evidenceCount).toBe(2);
  });

  it("points a one-way link from the file that contains it", () => {
    const [edge] = graphOf([link(PLAN, NOTES)]).edges;
    expect([edge.source, edge.target, edge.direction]).toEqual([
      PLAN.id,
      NOTES.id,
      "outgoing",
    ]);
  });

  it("keeps unconnected files as nodes", () => {
    const graph = graphOf([link(PLAN, NOTES)]);
    expect(graph.nodes.map((node) => node.name)).toEqual([
      "plan-copy.md",
      "notes.md",
      "grocery.md",
      "plan.md",
      "far.md",
    ]);
    expect(graph.nodes.find((n) => n.id === LONELY.id)?.degree).toBe(0);
  });

  it("never treats links or identical copies as inferred", () => {
    expect(edgeOrigin("documentLink")).toBe("confirmed");
    expect(edgeOrigin("contentHash")).toBe("confirmed");
    const graph = graphOf([link(PLAN, NOTES)], [copies(PLAN, COPY)]);
    for (const edge of graph.edges) {
      expect(edge.origin).toBe("confirmed");
      expect(edgeLabel(edge)).not.toMatch(/AI/);
    }
    for (const node of graph.nodes)
      expect(describeNode(node)).not.toMatch(/AI/);
  });

  it("has inferred edges only when the relationships contain model output", () => {
    const graph = graphOf([link(PLAN, NOTES), similar(NOTES, FAR)]);
    const inferred = graph.edges.filter((edge) => edge.origin === "inferred");
    expect(inferred.map((edge) => edge.kind)).toEqual(["similarity"]);
    expect(edgeLabel(inferred[0])).toBe("AI · Similar content");
    expect(describeNode(graph.nodes.find((n) => n.id === FAR.id)!)).toBe(
      "far.md, in research, 1 connection: 1 similar file found by AI",
    );
  });

  it("matches the list for the sample files: links and copies, nothing inferred", async () => {
    const documents = await loadFixtureDocuments();
    const relationships = discoverExplicitReferences(documents);
    const pairs = graphPairs(
      documents,
      relationships,
      localDuplicates(documents),
    );
    const graph = buildGraph(documents, pairs);
    expect(graph.nodes).toHaveLength(documents.length);
    expect(graph.edges).toHaveLength(pairs.length);
    expect(graph.edges.some((e) => e.kind === "exactDuplicate")).toBe(true);
    expect(graph.edges.some((e) => e.kind === "explicitReference")).toBe(true);
    expect(graph.edges.every((e) => e.origin === "confirmed")).toBe(true);
  });
});

describe("filters and neighbourhoods", () => {
  const graph = graphOf(
    [link(PLAN, NOTES), link(NOTES, FAR), similar(PLAN, FAR)],
    [copies(PLAN, COPY)],
  );

  it("hides a kind but keeps every file, with degrees recounted", () => {
    const shown = filterEdges(graph, new Set(["similarity"]));
    expect(shown.nodes).toHaveLength(DOCUMENTS.length);
    expect(shown.edges.some((e) => e.kind === "similarity")).toBe(false);
    expect(shown.nodes.find((n) => n.id === PLAN.id)?.kinds).toEqual({
      explicitReference: 1,
      exactDuplicate: 1,
    });
  });

  it("reaches files within the given number of connections", () => {
    const linksOnly = filterEdges(graph, new Set(["similarity"]));
    const ids = (g: GraphModel) => g.nodes.map((node) => node.id).sort();
    expect(ids(neighbourhood(linksOnly, COPY.id, 0))).toEqual([COPY.id]);
    expect(ids(neighbourhood(linksOnly, COPY.id, 1))).toEqual(
      [COPY.id, PLAN.id].sort(),
    );
    expect(ids(neighbourhood(linksOnly, COPY.id, 3))).toEqual(
      [COPY.id, PLAN.id, NOTES.id, FAR.id].sort(),
    );
  });

  it("shows every file until the folder is too large, then the selection's neighbourhood", () => {
    expect(mapSubset(graph, null, 5).omitted).toBe(0);
    const subset = mapSubset(graph, COPY.id, 4);
    expect(subset.centerId).toBe(COPY.id);
    expect(subset.graph.nodes.map((n) => n.id).sort()).toEqual(
      [COPY.id, PLAN.id, NOTES.id, FAR.id].sort(),
    );
    expect(subset.omitted).toBe(1);
    // A hub with more neighbours than fit keeps itself and the first by path.
    const tight = mapSubset(graph, PLAN.id, 2);
    expect(tight.graph.nodes.map((n) => n.id)).toContain(PLAN.id);
    expect(tight.graph.nodes).toHaveLength(2);
  });
});

import { describe, expect, it } from "vitest";
import { layoutBox, layoutGraph, type Point } from "./graphLayout";

const NODES = Array.from({ length: 24 }, (_, index) => ({
  id: `doc-${String(index).padStart(2, "0")}`,
}));
// A chain, a hub, a copy pair and a few unconnected files.
const EDGES = [
  ...NODES.slice(0, 8).map((node, index) => ({
    source: node.id,
    target: NODES[index + 1].id,
  })),
  ...NODES.slice(10, 16).map((node) => ({
    source: "doc-09",
    target: node.id,
  })),
  { source: "doc-17", target: "doc-18" },
];
const BOX = { width: 1000, height: 620 };

function shuffled<T>(items: T[]): T[] {
  // A fixed permutation, so the test itself is repeatable.
  return items
    .map((item, index) => [item, (index * 7919) % 101] as const)
    .sort((a, b) => a[1] - b[1])
    .map(([item]) => item);
}

function plain(positions: Map<string, Point>) {
  return [...positions.entries()].sort(([a], [b]) => (a < b ? -1 : 1));
}

describe("map layout", () => {
  it("gives the same positions for the same files", () => {
    expect(plain(layoutGraph(NODES, EDGES, BOX))).toEqual(
      plain(layoutGraph(NODES, EDGES, BOX)),
    );
  });

  it("does not depend on the order files and connections arrive in", () => {
    const reversedEdges = shuffled(EDGES).map(({ source, target }) => ({
      source: target,
      target: source,
    }));
    expect(plain(layoutGraph(shuffled(NODES), reversedEdges, BOX))).toEqual(
      plain(layoutGraph(NODES, EDGES, BOX)),
    );
  });

  it("places every file at a finite point inside the box", () => {
    const positions = layoutGraph(NODES, EDGES, BOX);
    expect(positions.size).toBe(NODES.length);
    for (const { x, y } of positions.values()) {
      expect(Number.isFinite(x) && Number.isFinite(y)).toBe(true);
      expect(x).toBeGreaterThanOrEqual(0);
      expect(x).toBeLessThanOrEqual(BOX.width);
      expect(y).toBeGreaterThanOrEqual(0);
      expect(y).toBeLessThanOrEqual(BOX.height);
    }
  });

  it("keeps a single file and an empty folder inside the box", () => {
    expect(layoutGraph([{ id: "only" }], [], BOX).get("only")).toEqual({
      x: 500,
      y: 310,
    });
    expect(layoutGraph([], [], BOX).size).toBe(0);
  });

  it("leaves a dragged file where it was put while the rest settle", () => {
    const before = layoutGraph(NODES, EDGES, BOX);
    const dropped = { x: 120, y: 90 };
    const after = layoutGraph(NODES, EDGES, {
      ...BOX,
      from: before,
      pinned: new Map([["doc-09", dropped]]),
      iterations: 30,
    });
    expect(after.get("doc-09")).toEqual(dropped);
    // Its neighbours moved towards it; unrelated files barely moved.
    const hubNeighbour = (map: Map<string, Point>) => map.get("doc-10")!;
    const distance = (a: Point, b: Point) => Math.hypot(a.x - b.x, a.y - b.y);
    expect(distance(hubNeighbour(after), dropped)).toBeLessThan(
      distance(hubNeighbour(before), dropped),
    );
  });

  it("grows the box with the number of files", () => {
    expect(layoutBox(4).width).toBe(800);
    expect(layoutBox(400).width).toBeGreaterThan(layoutBox(100).width);
  });
});

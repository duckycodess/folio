import {
  forceCollide,
  forceLink,
  forceManyBody,
  forceSimulation,
  forceX,
  forceY,
  type SimulationLinkDatum,
  type SimulationNodeDatum,
} from "d3-force";

export interface Point {
  x: number;
  y: number;
}

export interface LayoutOptions {
  width: number;
  height: number;
  /** Synchronous simulation steps; 300 settles a fresh layout. */
  iterations?: number;
  /** Nodes the user dragged; they keep these positions. */
  pinned?: ReadonlyMap<string, Point>;
  /**
   * Positions to continue from (after a drag). The result is then not refitted
   * to the box, so nothing the user placed jumps.
   */
  from?: ReadonlyMap<string, Point>;
  padding?: number;
}

interface LayoutNode extends SimulationNodeDatum {
  id: string;
}

function compare(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

/**
 * File names are wider than they are tall, so the simulation runs in a space
 * squeezed horizontally by this much and the result is stretched back.
 */
const LABEL_STRETCH = 1.5;

const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5));

/** A small linear congruential generator, so every run makes the same moves. */
function seededRandom(seed = 0x2f6b1d): () => number {
  let state = seed >>> 0;
  return () => {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    return state / 0x1_0000_0000;
  };
}

/** A box that leaves room for every file's label, for `layoutGraph`. */
export function layoutBox(nodeCount: number): {
  width: number;
  height: number;
} {
  const width = Math.max(800, Math.round(170 * Math.sqrt(nodeCount)));
  return { width, height: Math.round(width * 0.62) };
}

/**
 * Positions for a force-directed map, computed all at once: the same files and
 * connections always give the same picture, whatever order they arrive in, and
 * nothing animates (so reduced motion is respected by construction).
 */
export function layoutGraph(
  nodes: readonly { id: string }[],
  edges: readonly { source: string; target: string }[],
  {
    width,
    height,
    iterations = 300,
    pinned,
    from,
    padding = 48,
  }: LayoutOptions,
): Map<string, Point> {
  const ids = [...new Set(nodes.map((node) => node.id))].sort(compare);
  const known = new Set(ids);
  // A fresh layout is fitted to the box afterwards; a continued one stays put.
  const centerX = from ? width / 2 : 0;
  const centerY = from ? height / 2 : 0;
  const squeeze = (point: Point): Point => ({
    x: centerX + (point.x - centerX) / LABEL_STRETCH,
    y: point.y,
  });
  const data: LayoutNode[] = ids.map((id, index) => {
    const given = from?.get(id);
    const start = given && squeeze(given);
    // Phyllotaxis: evenly spread starting points without randomness.
    const radius = 18 * Math.sqrt(index + 0.5);
    const node: LayoutNode = start
      ? { id, x: start.x, y: start.y }
      : {
          id,
          x: centerX + Math.cos(index * GOLDEN_ANGLE) * radius,
          y: centerY + Math.sin(index * GOLDEN_ANGLE) * radius,
        };
    const pinPoint = pinned?.get(id);
    const pin = pinPoint && squeeze(pinPoint);
    if (pin) {
      node.x = node.fx = pin.x;
      node.y = node.fy = pin.y;
    }
    return node;
  });
  const links: SimulationLinkDatum<LayoutNode>[] = edges
    .filter(
      (edge) =>
        edge.source !== edge.target &&
        known.has(edge.source) &&
        known.has(edge.target),
    )
    .map((edge) =>
      edge.source < edge.target
        ? { source: edge.source, target: edge.target }
        : { source: edge.target, target: edge.source },
    )
    .sort(
      (a, b) =>
        compare(a.source as string, b.source as string) ||
        compare(a.target as string, b.target as string),
    );

  const simulation = forceSimulation(data)
    .randomSource(seededRandom())
    .force(
      "link",
      forceLink<LayoutNode, SimulationLinkDatum<LayoutNode>>(links)
        .id((node) => node.id)
        .distance(100),
    )
    .force("charge", forceManyBody().strength(-380).distanceMax(360))
    .force("collide", forceCollide(44))
    // Pull unconnected files in, so they sit near the rest instead of drifting.
    .force("x", forceX(centerX).strength(0.06))
    .force("y", forceY(centerY).strength(0.06))
    .stop();
  if (from) simulation.alpha(0.3);
  simulation.tick(iterations);

  const positions = new Map<string, Point>(
    data.map((node) => [
      node.id,
      pinned?.get(node.id) ?? {
        x: centerX + ((node.x ?? 0) - centerX) * LABEL_STRETCH,
        y: node.y ?? 0,
      },
    ]),
  );
  return from ? positions : fit(positions, width, height, padding);
}

/** Centres the layout in the box, shrinking (never stretching) it to fit. */
function fit(
  positions: Map<string, Point>,
  width: number,
  height: number,
  padding: number,
): Map<string, Point> {
  if (!positions.size) return positions;
  const points = [...positions.values()];
  const minX = Math.min(...points.map((p) => p.x));
  const maxX = Math.max(...points.map((p) => p.x));
  const minY = Math.min(...points.map((p) => p.y));
  const maxY = Math.max(...points.map((p) => p.y));
  const room = {
    x: Math.max(1, width - 2 * padding),
    y: Math.max(1, height - 2 * padding),
  };
  const scale = Math.min(
    1,
    room.x / Math.max(maxX - minX, 1e-9),
    room.y / Math.max(maxY - minY, 1e-9),
  );
  const offsetX = width / 2 - ((minX + maxX) / 2) * scale;
  const offsetY = height / 2 - ((minY + maxY) / 2) * scale;
  const fitted = new Map<string, Point>();
  for (const [id, point] of positions)
    fitted.set(id, {
      x: point.x * scale + offsetX,
      y: point.y * scale + offsetY,
    });
  return fitted;
}

/** The smallest box around the positions, for fitting the view to them. */
export function layoutBounds(positions: ReadonlyMap<string, Point>): {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
} {
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const { x, y } of positions.values()) {
    minX = Math.min(minX, x);
    minY = Math.min(minY, y);
    maxX = Math.max(maxX, x);
    maxY = Math.max(maxY, y);
  }
  return positions.size
    ? { minX, minY, maxX, maxY }
    : { minX: 0, minY: 0, maxX: 0, maxY: 0 };
}

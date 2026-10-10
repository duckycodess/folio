import type { GraphModel } from "../domain/graph";
import type { Point } from "../domain/graphLayout";

/**
 * What keyboard navigation needs to know about the map: every file in path
 * order, where each is drawn, and which files each one is connected to.
 */
export interface NavigationMap {
  order: readonly string[];
  positions: ReadonlyMap<string, Point>;
  neighbours: ReadonlyMap<string, readonly string[]>;
}

export function navigationMap(
  graph: GraphModel,
  positions: ReadonlyMap<string, Point>,
): NavigationMap {
  const neighbours = new Map<string, string[]>(
    graph.nodes.map((node) => [node.id, []]),
  );
  for (const edge of graph.edges) {
    const from = neighbours.get(edge.source);
    const to = neighbours.get(edge.target);
    if (!from || !to) continue;
    if (!from.includes(edge.target)) from.push(edge.target);
    if (!to.includes(edge.source)) to.push(edge.source);
  }
  return { order: graph.nodes.map((node) => node.id), positions, neighbours };
}

export interface GraphNavigationState {
  /** The node with the map's one Tab stop. */
  focusedId: string | null;
  /** The file open in the reader. */
  selectedId: string | null;
  /** Something to announce after this step, such as a key that went nowhere. */
  message: string | null;
}

export const NO_GRAPH_NAVIGATION: GraphNavigationState = {
  focusedId: null,
  selectedId: null,
  message: null,
};

export type GraphNavigationEvent =
  | { type: "focus"; id: string }
  | { type: "key"; key: string; map: NavigationMap }
  /** A click, or the reader opening or closing a file elsewhere. */
  | { type: "select"; id: string | null }
  | { type: "graphChanged"; previous: NavigationMap; map: NavigationMap };

/**
 * Opens the focused file's actions: Shift+F10 or the context-menu key, as in
 * file managers. Escape then closes the menu, and a second Escape deselects.
 */
export function isActionsKey(event: { key: string; shiftKey: boolean }) {
  return (event.key === "F10" && event.shiftKey) || event.key === "ContextMenu";
}

/** Keys the reducer handles; the view prevents their default action. */
export const NAVIGATION_KEYS = new Set([
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
  "Home",
  "End",
  "PageUp",
  "PageDown",
  "Enter",
  " ",
  "Escape",
]);

const DIRECTIONS: Record<string, Point> = {
  ArrowRight: { x: 1, y: 0 },
  ArrowLeft: { x: -1, y: 0 },
  // Screen coordinates: y grows downwards.
  ArrowUp: { x: 0, y: -1 },
  ArrowDown: { x: 0, y: 1 },
};

/** cos 60°: a neighbour counts if it lies within 60° of the arrow. */
const CONE = 0.5;

export const NOTHING_THAT_WAY = "No connected file that way";

/**
 * The connected file within 60° of the arrow's direction with the lowest
 * score, `distance / cos(angle)`: straight ahead counts at its distance, 45°
 * off at 1.41 times it and 60° off at twice it. A file straight ahead beats a
 * slightly nearer diagonal one, while a much nearer diagonal one still wins.
 * Ties go to the file closer to the arrow's line, then to the first by path.
 */
export function neighbourInDirection(
  map: NavigationMap,
  from: string,
  key: string,
): string | null {
  const direction = DIRECTIONS[key];
  const origin = map.positions.get(from);
  if (!direction || !origin) return null;
  let best: { id: string; score: number; cos: number; rank: number } | null =
    null;
  for (const id of map.neighbours.get(from) ?? []) {
    const point = map.positions.get(id);
    if (!point) continue;
    const dx = point.x - origin.x;
    const dy = point.y - origin.y;
    const distance = Math.hypot(dx, dy);
    if (distance === 0) continue;
    const cos = (dx * direction.x + dy * direction.y) / distance;
    if (cos < CONE) continue;
    const score = distance / cos;
    const rank = map.order.indexOf(id);
    if (
      !best ||
      score < best.score ||
      (score === best.score &&
        (cos > best.cos || (cos === best.cos && rank < best.rank)))
    )
      best = { id, score, cos, rank };
  }
  return best?.id ?? null;
}

function step(
  state: GraphNavigationState,
  key: string,
  map: NavigationMap,
): GraphNavigationState {
  const quiet = { ...state, message: null };
  const { order } = map;
  if (!order.length) return quiet;
  const index = state.focusedId ? order.indexOf(state.focusedId) : -1;
  // Before anything has focus, any movement starts at the first file.
  if (index < 0 && key !== "Escape")
    return key === "End"
      ? { ...quiet, focusedId: order[order.length - 1] }
      : { ...quiet, focusedId: order[0] };
  const focusedId = order[index];

  switch (key) {
    case "ArrowUp":
    case "ArrowDown":
    case "ArrowLeft":
    case "ArrowRight": {
      const next = neighbourInDirection(map, focusedId, key);
      return next
        ? { ...quiet, focusedId: next }
        : { ...quiet, message: NOTHING_THAT_WAY };
    }
    case "PageDown":
      return index < order.length - 1
        ? { ...quiet, focusedId: order[index + 1] }
        : { ...quiet, message: "This is the last file" };
    case "PageUp":
      return index > 0
        ? { ...quiet, focusedId: order[index - 1] }
        : { ...quiet, message: "This is the first file" };
    case "Home":
      return { ...quiet, focusedId: order[0] };
    case "End":
      return { ...quiet, focusedId: order[order.length - 1] };
    case "Enter":
    case " ":
      return { ...quiet, selectedId: focusedId };
    case "Escape":
      return { ...quiet, selectedId: null };
    default:
      return quiet;
  }
}

/** If the focused file left the map, a former neighbour, else the next by path. */
function survivor(
  id: string,
  previous: NavigationMap,
  map: NavigationMap,
): string | null {
  const present = new Set(map.order);
  const formerNeighbours = (previous.neighbours.get(id) ?? [])
    .filter((other) => present.has(other))
    .sort((a, b) => previous.order.indexOf(a) - previous.order.indexOf(b));
  if (formerNeighbours.length) return formerNeighbours[0];
  const index = previous.order.indexOf(id);
  for (let i = index + 1; i < previous.order.length; i++)
    if (present.has(previous.order[i])) return previous.order[i];
  for (let i = index - 1; i >= 0; i--)
    if (present.has(previous.order[i])) return previous.order[i];
  return map.order[0] ?? null;
}

export function graphNavigation(
  state: GraphNavigationState,
  event: GraphNavigationEvent,
): GraphNavigationState {
  switch (event.type) {
    case "focus":
      return { ...state, focusedId: event.id, message: null };
    case "key":
      return step(state, event.key, event.map);
    case "select":
      return {
        focusedId: event.id ?? state.focusedId,
        selectedId: event.id,
        message: null,
      };
    case "graphChanged": {
      const present = new Set(event.map.order);
      const focusedId =
        state.focusedId === null || present.has(state.focusedId)
          ? state.focusedId
          : survivor(state.focusedId, event.previous, event.map);
      const selectedId =
        state.selectedId !== null && present.has(state.selectedId)
          ? state.selectedId
          : null;
      return { focusedId, selectedId, message: null };
    }
  }
}

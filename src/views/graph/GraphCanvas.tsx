import { Maximize2, ZoomIn, ZoomOut } from "lucide-react";
import {
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type MouseEvent,
  type PointerEvent,
} from "react";
import {
  graphNavigation,
  isActionsKey,
  NAVIGATION_KEYS,
  navigationMap,
  type GraphNavigationState,
  type NavigationMap,
} from "../../app/graphNavigation";
import {
  ensureVisible,
  fitTo,
  panBy,
  toLayout,
  toScreen,
  zoomAt,
  zoomKey,
  ZOOM_STEP,
  type Size,
  type Viewport,
} from "../../app/viewport";
import {
  describeNode,
  edgeLabel,
  type GraphEdge,
  type GraphModel,
} from "../../domain/graph";
import {
  layoutBounds,
  layoutBox,
  layoutGraph,
  type Point,
} from "../../domain/graphLayout";
import { useAnnounce } from "../../ui/Announcer";

const NODE_RADIUS = 9;
/** Pointer movement below this many pixels is a click, not a drag. */
const DRAG_THRESHOLD = 4;
/** Below this zoom, only the selected file and its neighbours keep labels. */
const LABEL_ZOOM = 0.6;
const LABEL_LENGTH = 24;
/** Gap between parallel edges of different kinds between the same files. */
const EDGE_SPACING = 7;
/** Shorter edges go unlabelled (the list below the map has them); zooming in lengthens them. */
const LABEL_MIN_EDGE = 96;

function shortName(name: string): string {
  return name.length > LABEL_LENGTH
    ? `${name.slice(0, LABEL_LENGTH - 1)}…`
    : name;
}

type Gesture =
  | {
      kind: "node";
      id: string;
      pointerId: number;
      start: Point;
      origin: Point;
      moved: boolean;
    }
  | { kind: "pan"; pointerId: number; last: Point; moved: boolean }
  | { kind: "pinch"; distance: number };

interface Placed {
  edge: GraphEdge;
  from: Point;
  to: Point;
}

/** Screen endpoints, shortened to the node rims and spread apart when edges share a pair. */
function placeEdges(
  edges: readonly GraphEdge[],
  positions: ReadonlyMap<string, Point>,
  view: Viewport,
): Placed[] {
  const byPair = new Map<string, GraphEdge[]>();
  for (const edge of edges) {
    const key = [edge.source, edge.target].sort().join("|");
    const group = byPair.get(key);
    if (group) group.push(edge);
    else byPair.set(key, [edge]);
  }
  const placed: Placed[] = [];
  for (const group of byPair.values())
    group.forEach((edge, index) => {
      const a = positions.get(edge.source);
      const b = positions.get(edge.target);
      if (!a || !b) return;
      const from = toScreen(view, a);
      const to = toScreen(view, b);
      const dx = to.x - from.x;
      const dy = to.y - from.y;
      const length = Math.hypot(dx, dy) || 1;
      const ux = dx / length;
      const uy = dy / length;
      // The same offset for either direction, so parallel edges never cross.
      const sign = edge.source < edge.target ? 1 : -1;
      const offset = (index - (group.length - 1) / 2) * EDGE_SPACING * sign;
      const trim = Math.min(NODE_RADIUS + 3, length / 2);
      placed.push({
        edge,
        from: {
          x: from.x + ux * trim - uy * offset,
          y: from.y + uy * trim + ux * offset,
        },
        to: {
          x: to.x - ux * trim - uy * offset,
          y: to.y - uy * trim + ux * offset,
        },
      });
    });
  return placed;
}

interface GraphCanvasProps {
  /** Every file and connection on the map; the layout uses all of them, so filters never move files. */
  base: GraphModel;
  /** The same files, with only the connections the legend shows. */
  shown: GraphModel;
  selectedId: string | null;
  label: string;
  onOpen: (id: string) => void;
  onClose: () => void;
  /** Shift+F10, the context-menu key or a right-click on a node: its actions. */
  onActions?: (id: string) => void;
}

/**
 * The concept map: an SVG drawn by React, with one Tab stop. Arrow keys follow
 * connections, Page Up/Page Down go through every file by path, Enter opens
 * the file in the reader. Nothing animates; positions are computed up front.
 */
export function GraphCanvas({
  base,
  shown,
  selectedId,
  label,
  onOpen,
  onClose,
  onActions,
}: GraphCanvasProps) {
  const announce = useAnnounce();
  const ids = useId().replace(/[^a-zA-Z0-9_-]/g, "");
  const container = useRef<HTMLDivElement>(null);
  const svg = useRef<SVGSVGElement>(null);
  const nodeElements = useRef(new Map<string, SVGGElement>());
  const [size, setSize] = useState<Size | null>(null);

  const layout = useMemo(
    () => layoutGraph(base.nodes, base.edges, layoutBox(base.nodes.length)),
    [base],
  );
  // Drags apply to this layout only; a new set of files starts over.
  const [dragged, setDragged] = useState<{
    layout: Map<string, Point>;
    positions: Map<string, Point>;
    pinned: Map<string, Point>;
  } | null>(null);
  const positions = dragged?.layout === layout ? dragged.positions : layout;

  const [moved, setMoved] = useState<{
    layout: Map<string, Point>;
    viewport: Viewport;
  } | null>(null);
  const fitted = useMemo(
    () => (size ? fitTo(layoutBounds(layout), size) : null),
    [layout, size],
  );
  const viewport = moved?.layout === layout ? moved.viewport : fitted;
  function setViewport(next: Viewport) {
    setMoved({ layout, viewport: next });
  }

  const [nav, setNav] = useState<GraphNavigationState>({
    focusedId: selectedId,
    selectedId,
    message: null,
  });
  const map = useMemo(
    () => navigationMap(shown, positions),
    [shown, positions],
  );
  const previousMap = useRef<NavigationMap>(map);
  const focusPending = useRef(false);
  const mapHasFocus = useRef(false);

  // The reader opened or closed a file, here or elsewhere (Related, Back).
  useEffect(() => {
    setNav((state) =>
      state.selectedId === selectedId
        ? state
        : graphNavigation(state, { type: "select", id: selectedId }),
    );
  }, [selectedId]);

  useEffect(() => {
    if (previousMap.current === map) return;
    const previous = previousMap.current;
    previousMap.current = map;
    setNav((state) => {
      const next = graphNavigation(state, {
        type: "graphChanged",
        previous,
        map,
      });
      // The focused file left the map; keep keyboard focus inside it.
      if (next.focusedId !== state.focusedId && mapHasFocus.current)
        focusPending.current = true;
      return next;
    });
  }, [map]);

  useEffect(() => {
    if (!focusPending.current || !nav.focusedId) return;
    focusPending.current = false;
    nodeElements.current.get(nav.focusedId)?.focus({ preventScroll: true });
  }, [nav.focusedId]);

  useLayoutEffect(() => {
    const element = container.current;
    if (!element) return;
    const observer = new ResizeObserver(([entry]) => {
      const { width, height } = entry.contentRect;
      // A hidden map (the reader covers it in narrow windows) keeps its size.
      if (width > 0 && height > 0)
        setSize((current) =>
          current?.width === width && current.height === height
            ? current
            : { width, height },
        );
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  // Ctrl/⌘ + wheel zooms; a plain wheel scrolls the page. Chromium and WebKit
  // report a trackpad pinch as a wheel with ctrlKey, so pinching zooms too.
  // React's wheel listener is passive and can't prevent the browser's zoom.
  const latest = useRef({ viewport, layout });
  latest.current = { viewport, layout };
  useEffect(() => {
    const element = svg.current;
    if (!element) return;
    function onWheel(event: WheelEvent) {
      const current = latest.current.viewport;
      if (!current || !(event.ctrlKey || event.metaKey)) return;
      event.preventDefault();
      const rect = element!.getBoundingClientRect();
      // A pinch sends small deltas, a wheel notch large ones.
      const factor = Math.exp(
        -event.deltaY * (Math.abs(event.deltaY) < 50 ? 0.01 : 0.0015),
      );
      setMoved({
        layout: latest.current.layout,
        viewport: zoomAt(current, factor, {
          x: event.clientX - rect.left,
          y: event.clientY - rect.top,
        }),
      });
    }
    element.addEventListener("wheel", onWheel, { passive: false });
    return () => element.removeEventListener("wheel", onWheel);
  }, [size !== null]);

  const presentIds = new Set(map.order);
  const rovingId =
    (nav.focusedId && presentIds.has(nav.focusedId) && nav.focusedId) ||
    (selectedId && presentIds.has(selectedId) && selectedId) ||
    map.order[0];
  const neighbours = new Set(selectedId ? map.neighbours.get(selectedId) : []);

  function bringIntoView(id: string) {
    const point = positions.get(id);
    if (!point || !viewport || !size) return;
    const next = ensureVisible(viewport, point, size);
    if (next !== viewport) setViewport(next);
  }

  function apply(next: GraphNavigationState) {
    setNav(next);
    if (next.message) announce(next.message);
    if (next.focusedId && next.focusedId !== nav.focusedId) {
      focusPending.current = true;
      bringIntoView(next.focusedId);
    }
    if (next.selectedId !== nav.selectedId) {
      if (next.selectedId) onOpen(next.selectedId);
      else onClose();
    }
  }

  // Some browsers follow Shift+F10 or the context-menu key with a
  // `contextmenu` event too; the key already asked for the actions once.
  const actionsFromKey = useRef(false);

  function onKeyDown(event: KeyboardEvent<SVGSVGElement>) {
    if (event.altKey || event.ctrlKey || event.metaKey || !viewport || !size)
      return;
    if (isActionsKey(event)) {
      const id = nav.focusedId ?? nav.selectedId;
      if (!id || !onActions) return;
      event.preventDefault();
      actionsFromKey.current = true;
      onActions(id);
      return;
    }
    const zoomed = zoomKey(viewport, event.key, size, layoutBounds(positions));
    if (zoomed) {
      event.preventDefault();
      setViewport(zoomed);
      return;
    }
    if (!NAVIGATION_KEYS.has(event.key)) return;
    // With nothing open, Escape belongs to whatever else listens for it.
    if (event.key === "Escape" && !nav.selectedId) return;
    event.preventDefault();
    apply(graphNavigation(nav, { type: "key", key: event.key, map }));
  }

  const gesture = useRef<Gesture | null>(null);
  const pointers = useRef(new Map<number, Point>());
  const suppressClick = useRef(false);

  function local(event: PointerEvent): Point {
    const rect = svg.current!.getBoundingClientRect();
    return { x: event.clientX - rect.left, y: event.clientY - rect.top };
  }

  function pinchDistance(): { distance: number; center: Point } {
    const [a, b] = [...pointers.current.values()];
    return {
      distance: Math.hypot(a.x - b.x, a.y - b.y),
      center: { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 },
    };
  }

  function onPointerDown(event: PointerEvent<SVGSVGElement>) {
    // A right-click's `contextmenu` comes after this, so it always counts.
    actionsFromKey.current = false;
    if (!viewport || (event.pointerType === "mouse" && event.button !== 0))
      return;
    const point = local(event);
    pointers.current.set(event.pointerId, point);
    if (pointers.current.size === 2) {
      gesture.current = { kind: "pinch", distance: pinchDistance().distance };
      return;
    }
    const nodeId = (event.target as Element)
      .closest("[data-node-id]")
      ?.getAttribute("data-node-id");
    const origin = nodeId ? positions.get(nodeId) : undefined;
    gesture.current =
      nodeId && origin
        ? {
            kind: "node",
            id: nodeId,
            pointerId: event.pointerId,
            start: point,
            origin,
            moved: false,
          }
        : {
            kind: "pan",
            pointerId: event.pointerId,
            last: point,
            moved: false,
          };
  }

  function onPointerMove(event: PointerEvent<SVGSVGElement>) {
    const current = gesture.current;
    if (!current || !viewport) return;
    const point = local(event);
    if (pointers.current.has(event.pointerId))
      pointers.current.set(event.pointerId, point);
    if (current.kind === "pinch") {
      if (pointers.current.size < 2) return;
      const { distance, center } = pinchDistance();
      if (current.distance > 0)
        setViewport(zoomAt(viewport, distance / current.distance, center));
      current.distance = distance;
      return;
    }
    if (event.pointerId !== current.pointerId) return;
    if (current.kind === "pan") {
      const dx = point.x - current.last.x;
      const dy = point.y - current.last.y;
      if (!current.moved && Math.hypot(dx, dy) < DRAG_THRESHOLD) return;
      if (!current.moved) svg.current?.setPointerCapture(event.pointerId);
      current.moved = true;
      current.last = point;
      setViewport(panBy(viewport, dx, dy));
      return;
    }
    const dx = point.x - current.start.x;
    const dy = point.y - current.start.y;
    if (!current.moved && Math.hypot(dx, dy) < DRAG_THRESHOLD) return;
    if (!current.moved) svg.current?.setPointerCapture(event.pointerId);
    current.moved = true;
    const at = toLayout(viewport, point);
    const next = new Map(positions);
    next.set(current.id, at);
    const pinned = new Map(dragged?.layout === layout ? dragged.pinned : []);
    pinned.set(current.id, at);
    setDragged({ layout, positions: next, pinned });
  }

  function onPointerUp(event: PointerEvent<SVGSVGElement>) {
    pointers.current.delete(event.pointerId);
    const current = gesture.current;
    if (current?.kind === "pinch") {
      if (!pointers.current.size) gesture.current = null;
      return;
    }
    if (!current || current.pointerId !== event.pointerId) return;
    gesture.current = null;
    if (!current.moved) return;
    suppressClick.current = true;
    if (current.kind !== "node" || dragged?.layout !== layout) return;
    // Let the neighbours of the dropped file settle around it; still no animation.
    setDragged({
      layout,
      pinned: dragged.pinned,
      positions: layoutGraph(base.nodes, base.edges, {
        ...layoutBox(base.nodes.length),
        from: dragged.positions,
        pinned: dragged.pinned,
        iterations: 30,
      }),
    });
  }

  function onClick(event: MouseEvent<SVGSVGElement>) {
    if (suppressClick.current) {
      suppressClick.current = false;
      return;
    }
    const id = (event.target as Element)
      .closest("[data-node-id]")
      ?.getAttribute("data-node-id");
    if (!id) return;
    nodeElements.current.get(id)?.focus({ preventScroll: true });
    apply({ focusedId: id, selectedId: id, message: null });
  }

  function fitMap() {
    if (size) setViewport(fitTo(layoutBounds(positions), size));
  }

  function zoomBy(factor: number) {
    if (viewport && size)
      setViewport(
        zoomAt(viewport, factor, { x: size.width / 2, y: size.height / 2 }),
      );
  }

  const placed = viewport ? placeEdges(shown.edges, positions, viewport) : [];
  const showAllLabels = (viewport?.k ?? 1) >= LABEL_ZOOM;
  const marker = (active: boolean) =>
    `url(#${ids}-arrow${active ? "-active" : ""})`;

  return (
    <div className="graph-canvas-frame">
      <div className="graph-canvas" ref={container}>
        {size && viewport && (
          <svg
            ref={svg}
            className={`graph-svg${selectedId ? " has-selection" : ""}`}
            width={size.width}
            height={size.height}
            role="group"
            aria-label={label}
            aria-describedby={`${ids}-hint`}
            onKeyDown={onKeyDown}
            onPointerDown={onPointerDown}
            onPointerMove={onPointerMove}
            onPointerUp={onPointerUp}
            onPointerCancel={onPointerUp}
            onClick={onClick}
            onContextMenu={(event) => {
              if (actionsFromKey.current) {
                actionsFromKey.current = false;
                event.preventDefault();
                return;
              }
              const id = (event.target as Element)
                .closest("[data-node-id]")
                ?.getAttribute("data-node-id");
              if (!id || !onActions) return;
              event.preventDefault();
              nodeElements.current.get(id)?.focus({ preventScroll: true });
              onActions(id);
            }}
            onFocus={() => (mapHasFocus.current = true)}
            onBlur={(event) => {
              if (!event.currentTarget.contains(event.relatedTarget as Node))
                mapHasFocus.current = false;
            }}
          >
            <defs>
              {[false, true].map((active) => (
                <marker
                  key={String(active)}
                  id={`${ids}-arrow${active ? "-active" : ""}`}
                  viewBox="0 0 10 10"
                  refX="9"
                  refY="5"
                  markerWidth="9"
                  markerHeight="9"
                  markerUnits="userSpaceOnUse"
                  orient="auto-start-reverse"
                >
                  <path
                    d="M0,0 L10,5 L0,10 z"
                    className={`graph-arrow${active ? " is-active" : ""}`}
                  />
                </marker>
              ))}
            </defs>
            <g aria-hidden="true">
              {placed.map(({ edge, from, to }) => {
                const active =
                  edge.source === selectedId || edge.target === selectedId;
                const className = `graph-edge graph-edge-${edge.kind} is-${edge.origin}${active ? " is-active" : ""}`;
                if (edge.kind === "exactDuplicate")
                  return (
                    <g key={edge.id} className={className}>
                      <line
                        className="graph-edge-outer"
                        x1={from.x}
                        y1={from.y}
                        x2={to.x}
                        y2={to.y}
                      />
                      <line
                        className="graph-edge-inner"
                        x1={from.x}
                        y1={from.y}
                        x2={to.x}
                        y2={to.y}
                      />
                    </g>
                  );
                const arrows = edge.kind === "explicitReference";
                return (
                  <line
                    key={edge.id}
                    className={className}
                    x1={from.x}
                    y1={from.y}
                    x2={to.x}
                    y2={to.y}
                    markerEnd={arrows ? marker(active) : undefined}
                    markerStart={
                      arrows && edge.direction === "mutual"
                        ? marker(active)
                        : undefined
                    }
                  />
                );
              })}
              {placed.map(({ edge, from, to }) => {
                const active =
                  edge.source === selectedId || edge.target === selectedId;
                if (!active && edge.origin !== "inferred") return null;
                if (Math.hypot(to.x - from.x, to.y - from.y) < LABEL_MIN_EDGE)
                  return null;
                return (
                  <text
                    key={`label-${edge.id}`}
                    className={`graph-edge-label${active ? " is-active" : ""}`}
                    x={(from.x + to.x) / 2}
                    y={(from.y + to.y) / 2}
                    dy="0.35em"
                    textAnchor="middle"
                  >
                    {active ? edgeLabel(edge) : "AI"}
                  </text>
                );
              })}
            </g>
            <g>
              {shown.nodes.map((node) => {
                const point = positions.get(node.id);
                if (!point) return null;
                const at = toScreen(viewport, point);
                const selected = node.id === selectedId;
                const labelled =
                  showAllLabels ||
                  selected ||
                  node.id === nav.focusedId ||
                  neighbours.has(node.id);
                return (
                  <g
                    key={node.id}
                    ref={(element) => {
                      if (element) nodeElements.current.set(node.id, element);
                      else nodeElements.current.delete(node.id);
                    }}
                    className={`graph-node${selected ? " is-selected" : ""}${neighbours.has(node.id) ? " is-neighbour" : ""}`}
                    transform={`translate(${at.x} ${at.y})`}
                    role="button"
                    tabIndex={node.id === rovingId ? 0 : -1}
                    aria-label={describeNode(node)}
                    aria-pressed={selected}
                    data-node-id={node.id}
                    data-document-id={node.id}
                    onFocus={() =>
                      setNav((state) =>
                        state.focusedId === node.id
                          ? state
                          : graphNavigation(state, {
                              type: "focus",
                              id: node.id,
                            }),
                      )
                    }
                  >
                    <title>{node.relativePath}</title>
                    <circle className="graph-node-hit" r={NODE_RADIUS + 8} />
                    <circle className="graph-focus-ring" r={NODE_RADIUS + 5} />
                    <circle className="graph-node-dot" r={NODE_RADIUS} />
                    {labelled && (
                      <text
                        className="graph-node-label"
                        y={NODE_RADIUS + 15}
                        textAnchor="middle"
                      >
                        {shortName(node.name)}
                      </text>
                    )}
                  </g>
                );
              })}
            </g>
          </svg>
        )}
        <div className="graph-zoom">
          <button
            type="button"
            className="icon-button"
            aria-label="Zoom in"
            title="Zoom in (+)"
            onClick={() => zoomBy(ZOOM_STEP)}
          >
            <ZoomIn size={18} aria-hidden="true" />
          </button>
          <button
            type="button"
            className="icon-button"
            aria-label="Zoom out"
            title="Zoom out (−)"
            onClick={() => zoomBy(1 / ZOOM_STEP)}
          >
            <ZoomOut size={18} aria-hidden="true" />
          </button>
          <button
            type="button"
            className="icon-button"
            aria-label="Fit the map"
            title="Fit the map (0)"
            onClick={fitMap}
          >
            <Maximize2 size={18} aria-hidden="true" />
          </button>
        </div>
      </div>
      <p id={`${ids}-hint`} className="graph-hint">
        Arrow keys follow connections. Page Up and Page Down go through every
        file. Enter opens a file. + and − zoom; 0 fits the map. Ctrl or ⌘ with
        the scroll wheel, or a pinch, zooms too. Drag to move the map or a file.
      </p>
    </div>
  );
}

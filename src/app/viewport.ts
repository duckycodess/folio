import type { Point } from "../domain/graphLayout";

/**
 * Pan and zoom of the map: a layout point `p` is drawn at `p * k + (x, y)`.
 * Only positions scale; nodes and labels keep their size, so text stays
 * readable at every zoom level.
 */
export interface Viewport {
  x: number;
  y: number;
  k: number;
}

export interface Size {
  width: number;
  height: number;
}

export interface Bounds {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
}

export const MIN_SCALE = 0.25;
export const MAX_SCALE = 4;
export const FIT_MAX_SCALE = 1.6;
/** One press of + or −. */
export const ZOOM_STEP = 1.25;

export const IDENTITY: Viewport = { x: 0, y: 0, k: 1 };

export function clampScale(k: number): number {
  if (!Number.isFinite(k)) return 1;
  return Math.min(MAX_SCALE, Math.max(MIN_SCALE, k));
}

export function toScreen(view: Viewport, point: Point): Point {
  return { x: point.x * view.k + view.x, y: point.y * view.k + view.y };
}

export function toLayout(view: Viewport, point: Point): Point {
  return { x: (point.x - view.x) / view.k, y: (point.y - view.y) / view.k };
}

export function panBy(view: Viewport, dx: number, dy: number): Viewport {
  return { ...view, x: view.x + dx, y: view.y + dy };
}

/** Zooms by `factor` around a screen point, which stays where it is. */
export function zoomAt(
  view: Viewport,
  factor: number,
  anchor: Point,
): Viewport {
  const k = clampScale(view.k * factor);
  const fixed = toLayout(view, anchor);
  return { k, x: anchor.x - fixed.x * k, y: anchor.y - fixed.y * k };
}

/**
 * Room kept around the outermost files: file names are drawn centred under
 * their node, so they need more space to the sides than above and below.
 * `y` covers the label below the bottom-most node, not just the node itself
 * (#67 item 7: a bottom node's label was getting clipped).
 */
export const FIT_PADDING: Point = { x: 88, y: 56 };

/**
 * Shows all of `bounds` centred in `size`. A small map is spread out to at
 * most `FIT_MAX_SCALE`; only positions scale, so labels keep their size.
 */
export function fitTo(
  bounds: Bounds,
  size: Size,
  padding: Point = FIT_PADDING,
): Viewport {
  const width = Math.max(bounds.maxX - bounds.minX, 1);
  const height = Math.max(bounds.maxY - bounds.minY, 1);
  const k = clampScale(
    Math.min(
      FIT_MAX_SCALE,
      Math.max(size.width - 2 * padding.x, 1) / width,
      Math.max(size.height - 2 * padding.y, 1) / height,
    ),
  );
  return {
    k,
    x: size.width / 2 - ((bounds.minX + bounds.maxX) / 2) * k,
    y: size.height / 2 - ((bounds.minY + bounds.maxY) / 2) * k,
  };
}

/** Pans as little as needed so a layout point is at least `margin` inside. */
export function ensureVisible(
  view: Viewport,
  point: Point,
  size: Size,
  margin = 48,
): Viewport {
  const screen = toScreen(view, point);
  function shift(position: number, extent: number): number {
    // A view too small for both margins centres the point instead.
    if (extent < 2 * margin) return extent / 2 - position;
    if (position < margin) return margin - position;
    if (position > extent - margin) return extent - margin - position;
    return 0;
  }
  const dx = shift(screen.x, size.width);
  const dy = shift(screen.y, size.height);
  return dx || dy ? panBy(view, dx, dy) : view;
}

/**
 * The map's zoom keys: + and − zoom around the centre, 0 fits everything.
 * Returns `null` for any other key.
 */
export function zoomKey(
  view: Viewport,
  key: string,
  size: Size,
  bounds: Bounds,
): Viewport | null {
  const center = { x: size.width / 2, y: size.height / 2 };
  switch (key) {
    case "+":
    case "=":
      return zoomAt(view, ZOOM_STEP, center);
    case "-":
    case "_":
      return zoomAt(view, 1 / ZOOM_STEP, center);
    case "0":
      return fitTo(bounds, size);
    default:
      return null;
  }
}

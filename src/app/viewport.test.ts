import { describe, expect, it } from "vitest";
import {
  clampScale,
  ensureVisible,
  FIT_MAX_SCALE,
  fitTo,
  IDENTITY,
  MAX_SCALE,
  MIN_SCALE,
  toLayout,
  toScreen,
  zoomAt,
  zoomKey,
} from "./viewport";

const SIZE = { width: 600, height: 400 };

describe("map viewport", () => {
  it("keeps the scale between 25% and 400%", () => {
    expect(clampScale(10)).toBe(MAX_SCALE);
    expect(clampScale(0.01)).toBe(MIN_SCALE);
    expect(clampScale(Number.NaN)).toBe(1);
    let view = IDENTITY;
    for (let i = 0; i < 20; i++) view = zoomAt(view, 2, { x: 0, y: 0 });
    expect(view.k).toBe(MAX_SCALE);
  });

  it("zooms around the pointer, which stays over the same point", () => {
    const start = { x: 30, y: -20, k: 1.5 };
    const anchor = { x: 210, y: 140 };
    const under = toLayout(start, anchor);
    const zoomed = zoomAt(start, 1.8, anchor);
    expect(zoomed.k).toBeCloseTo(2.7);
    const after = toScreen(zoomed, under);
    expect(after.x).toBeCloseTo(anchor.x);
    expect(after.y).toBeCloseTo(anchor.y);
  });

  it("fits a layout into the view, centred, spreading a small one a little", () => {
    const small = fitTo({ minX: 100, minY: 100, maxX: 200, maxY: 150 }, SIZE);
    expect(small.k).toBe(FIT_MAX_SCALE);
    expect(toScreen(small, { x: 150, y: 125 })).toEqual({ x: 300, y: 200 });
    const large = fitTo({ minX: 0, minY: 0, maxX: 2000, maxY: 400 }, SIZE, {
      x: 0,
      y: 0,
    });
    expect(large.k).toBeCloseTo(0.3);
  });

  it("pans just enough to bring a point into view", () => {
    const view = { x: 0, y: 0, k: 1 };
    expect(ensureVisible(view, { x: 300, y: 200 }, SIZE)).toBe(view);
    const moved = ensureVisible(view, { x: 700, y: -10 }, SIZE, 40);
    expect(toScreen(moved, { x: 700, y: -10 })).toEqual({ x: 560, y: 40 });
  });

  it("maps +, − and 0 to zoom in, out and fit", () => {
    const bounds = { minX: 0, minY: 0, maxX: 100, maxY: 100 };
    expect(zoomKey(IDENTITY, "+", SIZE, bounds)?.k).toBe(1.25);
    expect(zoomKey(IDENTITY, "-", SIZE, bounds)?.k).toBe(0.8);
    expect(zoomKey({ x: 9, y: 9, k: 3 }, "0", SIZE, bounds)).toEqual(
      fitTo(bounds, SIZE),
    );
    expect(zoomKey(IDENTITY, "a", SIZE, bounds)).toBeNull();
  });
});

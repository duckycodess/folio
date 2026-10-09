import { describe, expect, it } from "vitest";
import {
  graphNavigation,
  NO_GRAPH_NAVIGATION,
  NOTHING_THAT_WAY,
  type GraphNavigationEvent,
  type GraphNavigationState,
  type NavigationMap,
} from "./graphNavigation";

/**
 *            north
 *              |
 *   west --- center --- east        lonely (no connections)
 *              \
 *               south-east (45° below the line to east)
 */
function map(
  positions: Record<string, [number, number]>,
  links: [string, string][],
): NavigationMap {
  const neighbours = new Map<string, string[]>(
    Object.keys(positions).map((id) => [id, []]),
  );
  for (const [a, b] of links) {
    neighbours.get(a)!.push(b);
    neighbours.get(b)!.push(a);
  }
  return {
    order: Object.keys(positions).sort(),
    positions: new Map(
      Object.entries(positions).map(([id, [x, y]]) => [id, { x, y }]),
    ),
    neighbours,
  };
}

const MAP = map(
  {
    center: [0, 0],
    east: [100, 0],
    west: [-100, 0],
    north: [0, -100],
    southeast: [60, 60],
    lonely: [300, 300],
  },
  [
    ["center", "east"],
    ["center", "west"],
    ["center", "north"],
    ["center", "southeast"],
  ],
);
// Path order: center, east, lonely, north, southeast, west.

function from(focusedId: string | null, selectedId: string | null = null) {
  return { ...NO_GRAPH_NAVIGATION, focusedId, selectedId };
}

function press(state: GraphNavigationState, ...keys: string[]) {
  return keys.reduce(
    (current, key) => graphNavigation(current, { type: "key", key, map: MAP }),
    state,
  );
}

describe("moving through the map with arrow keys", () => {
  it("goes to the connected file in the arrow's direction", () => {
    expect(press(from("center"), "ArrowLeft").focusedId).toBe("west");
    expect(press(from("center"), "ArrowUp").focusedId).toBe("north");
    // South-east is 45° from both right and down; down has only it.
    expect(press(from("center"), "ArrowDown").focusedId).toBe("southeast");
  });

  it("prefers the file straight ahead over a slightly nearer diagonal one", () => {
    // East: 100 away at 0° scores 100. South-east: 85 away at 45° scores 120.
    expect(press(from("center"), "ArrowRight").focusedId).toBe("east");
  });

  it("still picks a much nearer diagonal file over a distant straight one", () => {
    const near = map(
      { center: [0, 0], farEast: [300, 0], nearDiagonal: [30, 30] },
      [
        ["center", "farEast"],
        ["center", "nearDiagonal"],
      ],
    );
    // Near diagonal: 42 away at 45° scores 60, far east scores 300.
    const state = graphNavigation(from("center"), {
      type: "key",
      key: "ArrowRight",
      map: near,
    });
    expect(state.focusedId).toBe("nearDiagonal");
  });

  it("only follows connections", () => {
    // North has no connection to the west file, though it lies that way.
    expect(press(from("north"), "ArrowLeft")).toEqual({
      ...from("north"),
      message: NOTHING_THAT_WAY,
    });
  });

  it("stays put and says so when nothing is connected that way", () => {
    const state = press(from("west"), "ArrowLeft");
    expect(state.focusedId).toBe("west");
    expect(state.message).toBe(NOTHING_THAT_WAY);
    // The next successful move clears the message.
    expect(press(state, "ArrowRight").message).toBeNull();
  });
});

describe("moving through every file by path", () => {
  it("reaches an unconnected file with Page Down", () => {
    expect(press(from("east"), "PageDown").focusedId).toBe("lonely");
    expect(press(from("lonely"), "PageUp").focusedId).toBe("east");
  });

  it("stops at either end and says so", () => {
    expect(press(from("west"), "PageDown")).toMatchObject({
      focusedId: "west",
      message: "This is the last file",
    });
    expect(press(from("center"), "PageUp").message).toBe(
      "This is the first file",
    );
  });

  it("jumps to the first and last file with Home and End", () => {
    expect(press(from("north"), "Home").focusedId).toBe("center");
    expect(press(from("north"), "End").focusedId).toBe("west");
  });

  it("starts at the first file when nothing had focus", () => {
    expect(press(from(null), "ArrowRight").focusedId).toBe("center");
  });
});

describe("selecting", () => {
  it("opens the focused file with Enter and clears it with Escape", () => {
    const opened = press(from("north"), "Enter");
    expect(opened.selectedId).toBe("north");
    const cleared = press(opened, "Escape");
    expect(cleared).toEqual(from("north"));
  });

  it("follows a file opened elsewhere and keeps focus on a close", () => {
    const opened = graphNavigation(from("center"), {
      type: "select",
      id: "east",
    });
    expect(opened).toEqual(from("east", "east"));
    expect(graphNavigation(opened, { type: "select", id: null })).toEqual(
      from("east"),
    );
  });
});

describe("when the map changes", () => {
  function change(
    state: GraphNavigationState,
    next: NavigationMap,
  ): GraphNavigationState {
    const event: GraphNavigationEvent = {
      type: "graphChanged",
      previous: MAP,
      map: next,
    };
    return graphNavigation(state, event);
  }

  it("moves focus off a removed file to a former neighbour", () => {
    const withoutCenter = map(
      { east: [100, 0], west: [-100, 0], lonely: [300, 300] },
      [],
    );
    expect(change(from("center", "center"), withoutCenter)).toEqual(
      from("east"),
    );
  });

  it("falls back to the next file by path for a removed unconnected file", () => {
    const withoutLonely = map(
      { center: [0, 0], north: [0, -100], west: [-100, 0] },
      [["center", "north"]],
    );
    expect(change(from("lonely"), withoutLonely).focusedId).toBe("north");
  });

  it("keeps focus and selection on files that are still there", () => {
    expect(change(from("west", "east"), MAP)).toEqual(from("west", "east"));
  });
});

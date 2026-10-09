import { describe, expect, it } from "vitest";
import type { DocumentRecord } from "./contracts";
import {
  foldersOf,
  hasFilters,
  NO_FILTERS,
  passesFilters,
  rememberRecent,
  togglePin,
  type HomeFilters,
} from "./homeFilters";

const NOW = new Date(2026, 9, 10, 15, 0).getTime();
const DAY = 24 * 60 * 60 * 1000;
const doc = (
  relativePath: string,
  mediaType: DocumentRecord["mediaType"],
  modifiedAtMs?: number,
) =>
  ({
    id: relativePath,
    relativePath,
    name: relativePath.split("/").at(-1),
    mediaType,
    modifiedAtMs,
  }) as DocumentRecord;

const files = [
  doc(
    "Research/Interviews/panayam.md",
    "text/markdown",
    NOW - 2 * 60 * 60 * 1000,
  ),
  doc("Research/consent.pdf", "application/pdf", NOW - 3 * DAY),
  doc("Research Notes/tala.txt", "text/plain", NOW - 20 * DAY),
  doc("budget.md", "text/markdown", NOW - 60 * DAY),
  doc("Downloads/walang-petsa.txt", "text/plain"),
];
const pass = (filters: Partial<HomeFilters>) =>
  files
    .filter((file) => passesFilters(file, { ...NO_FILTERS, ...filters }, NOW))
    .map((file) => file.relativePath);

describe("Home filters", () => {
  it("passes everything without filters", () => {
    expect(pass({})).toHaveLength(files.length);
    expect(hasFilters(NO_FILTERS)).toBe(false);
  });

  it("includes subfolders but not folders that only share a prefix", () => {
    expect(pass({ folder: "Research" })).toEqual([
      "Research/Interviews/panayam.md",
      "Research/consent.pdf",
    ]);
    expect(pass({ folder: "" })).toEqual(["budget.md"]);
  });

  it("filters by file type", () => {
    expect(pass({ type: "application/pdf" })).toEqual(["Research/consent.pdf"]);
  });

  it("filters by modified date and never guesses a missing date", () => {
    expect(pass({ modified: "today" })).toEqual([
      "Research/Interviews/panayam.md",
    ]);
    expect(pass({ modified: "week" })).toHaveLength(2);
    expect(pass({ modified: "month" })).toHaveLength(3);
    expect(pass({ modified: "month" })).not.toContain(
      "Downloads/walang-petsa.txt",
    );
  });

  it("combines filters", () => {
    expect(
      pass({ folder: "Research", type: "text/markdown", modified: "week" }),
    ).toEqual(["Research/Interviews/panayam.md"]);
  });

  it("lists every folder that holds a file, with its parents", () => {
    expect(foldersOf(files)).toEqual([
      "Downloads",
      "Research",
      "Research Notes",
      "Research/Interviews",
    ]);
  });
});

describe("Home preferences", () => {
  it("keeps recent files newest first, without repeats, up to a limit", () => {
    expect(rememberRecent(["b", "a"], "a")).toEqual(["a", "b"]);
    expect(rememberRecent(["c", "b", "a"], "d", 3)).toEqual(["d", "c", "b"]);
  });

  it("pins and unpins folders", () => {
    expect(togglePin([], "Research")).toEqual(["Research"]);
    expect(togglePin(["Research", "Downloads"], "Research")).toEqual([
      "Downloads",
    ]);
  });
});

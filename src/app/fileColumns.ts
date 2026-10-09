/**
 * Which of the file table's columns fit at a given width, and in what order
 * they give way. Size drops first, then Modified, then Type, then Location
 * (#67): the file name is never the column that gets crushed, because it's
 * the one column these never touch.
 */

export type FileColumn = "location" | "type" | "modified" | "size";

/** Drop order: the first entry is the first column to go. */
export const COLUMN_DROP_ORDER: FileColumn[] = [
  "size",
  "modified",
  "type",
  "location",
];

/** Roughly what each column needs to show its longest usual value. */
export const COLUMN_WIDTH: Record<FileColumn, number> = {
  location: 160,
  type: 96,
  modified: 112,
  size: 80,
};

/** Gap between columns, matching `--space-4` in tokens.css. */
export const COLUMN_GAP = 16;

/** The name column's own minimum, below which it would start truncating hard. */
export const NAME_MIN_WIDTH = 160;

function widthFor(columns: FileColumn[]): number {
  const gaps = (columns.length + 1) * COLUMN_GAP;
  return (
    NAME_MIN_WIDTH +
    gaps +
    columns.reduce((total, column) => total + COLUMN_WIDTH[column], 0)
  );
}

const ALL_COLUMNS: FileColumn[] = ["location", "type", "modified", "size"];

/**
 * The columns that fit a table of `containerWidth`, keeping the name column
 * at its minimum and dropping the lowest-priority column first until the
 * rest fit.
 */
export function visibleColumns(containerWidth: number): FileColumn[] {
  let shown = ALL_COLUMNS;
  for (const candidate of COLUMN_DROP_ORDER) {
    if (widthFor(shown) <= containerWidth) break;
    shown = shown.filter((column) => column !== candidate);
  }
  return shown;
}

const COLUMN_HEAD_ORDER: FileColumn[] = [
  "location",
  "type",
  "modified",
  "size",
];

/** The grid template for the table head and each row, name column first. */
export function fileColumnsTemplate(shown: FileColumn[]): string {
  const tracks = COLUMN_HEAD_ORDER.filter((column) =>
    shown.includes(column),
  ).map((column) => `${COLUMN_WIDTH[column]}px`);
  return ["minmax(0, 1fr)", ...tracks].join(" ");
}

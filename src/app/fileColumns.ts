/**
 * Which of the file table's columns fit at a given width, and in what order
 * they give way. The brandkit mockup's table has Name, Folder and Modified;
 * Modified drops first, then Folder, which moves under the name (#67): the
 * file name is never the column that gets crushed.
 */

export type FileColumn = "location" | "modified";

/** Drop order: the first entry is the first column to go. */
export const COLUMN_DROP_ORDER: FileColumn[] = ["modified", "location"];

/** Roughly what each column needs to show its longest usual value. */
export const COLUMN_WIDTH: Record<FileColumn, number> = {
  location: 140,
  modified: 110,
};

/** Gap between columns: the mockup's 10px cell padding on each side. */
export const COLUMN_GAP = 20;

/** The name column's own minimum, below which it would start truncating hard. */
export const NAME_MIN_WIDTH = 160;
/**
 * The most room the name asks for before other columns give way: past this,
 * a very long name truncates rather than pushing every other column out.
 */
export const NAME_MAX_WIDTH = 320;

/**
 * What the name column needs: the longest name's natural width, kept
 * between NAME_MIN_WIDTH and NAME_MAX_WIDTH.
 */
export function nameColumnWidth(longestName: number): number {
  if (!Number.isFinite(longestName)) return NAME_MIN_WIDTH;
  return Math.min(NAME_MAX_WIDTH, Math.max(NAME_MIN_WIDTH, longestName));
}

function widthFor(columns: FileColumn[], nameWidth: number): number {
  const gaps = (columns.length + 1) * COLUMN_GAP;
  return (
    nameWidth +
    gaps +
    columns.reduce((total, column) => total + COLUMN_WIDTH[column], 0)
  );
}

const ALL_COLUMNS: FileColumn[] = ["location", "modified"];

/**
 * The columns that fit a row of `rowWidth` (the grid's own width, after the
 * row's padding and ⋯ menu), keeping `nameWidth` for the name and dropping
 * the lowest-priority column first until the rest fit.
 */
export function visibleColumns(
  rowWidth: number,
  nameWidth: number = NAME_MIN_WIDTH,
): FileColumn[] {
  let shown = ALL_COLUMNS;
  for (const candidate of COLUMN_DROP_ORDER) {
    if (widthFor(shown, nameWidth) <= rowWidth) break;
    shown = shown.filter((column) => column !== candidate);
  }
  return shown;
}

const COLUMN_HEAD_ORDER: FileColumn[] = ["location", "modified"];

/** The grid template for the table head and each row, name column first. */
export function fileColumnsTemplate(shown: FileColumn[]): string {
  const tracks = COLUMN_HEAD_ORDER.filter((column) =>
    shown.includes(column),
  ).map((column) => `${COLUMN_WIDTH[column]}px`);
  return ["minmax(0, 1fr)", ...tracks].join(" ");
}

/**
 * Line diffs for the exact preview of a text edit. The diff is only a way of
 * showing the change: the operation always carries the full new text, and a
 * diff too large to compute is reported as such so the UI can show that text
 * in full instead of an approximation.
 */

/** How a line ends. `""` is the last line of a file without a final line break. */
export type LineEnding = "\n" | "\r\n" | "\r" | "";

export interface DiffLine {
  kind: "same" | "added" | "removed";
  /** The line without its line ending. */
  text: string;
  ending: LineEnding;
  /** 1-based line number in the current text; absent for an added line. */
  beforeLine?: number;
  /** 1-based line number in the new text; absent for a removed line. */
  afterLine?: number;
}

/** A run of changed lines with up to `context` unchanged lines either side. */
export interface DiffHunk {
  lines: DiffLine[];
}

export type LineDiff =
  { tooLarge: false; hunks: DiffHunk[] } | { tooLarge: true };

/**
 * The comparison table is bounded so a large rewrite can't stall the window.
 * Unchanged lines at the start and end are trimmed first and don't count.
 */
export const DEFAULT_MAX_CELLS = 1_000_000;

interface Line {
  text: string;
  ending: LineEnding;
}

/**
 * Splits text into lines that keep their exact endings, so joining them gives
 * the text back byte for byte. Text ending in a line break has no empty last
 * line; empty text has no lines.
 */
export function splitLines(text: string): Line[] {
  const lines: Line[] = [];
  const pattern = /\r\n|\n|\r/g;
  let start = 0;
  for (const match of text.matchAll(pattern)) {
    lines.push({
      text: text.slice(start, match.index),
      ending: match[0] as LineEnding,
    });
    start = match.index + match[0].length;
  }
  if (start < text.length) lines.push({ text: text.slice(start), ending: "" });
  return lines;
}

function sameLine(a: Line, b: Line): boolean {
  return a.text === b.text && a.ending === b.ending;
}

type Op = { kind: DiffLine["kind"]; before?: number; after?: number };

/**
 * The shortest edit between two line lists, as 0-based indexes, or `null` when
 * the comparison would exceed `maxCells`. Common leading and trailing lines
 * are matched directly; only the middle is compared with a longest-common-
 * subsequence table.
 */
function lineOps(
  before: Line[],
  after: Line[],
  maxCells: number,
  same: (a: Line, b: Line) => boolean = sameLine,
): Op[] | null {
  let head = 0;
  while (
    head < before.length &&
    head < after.length &&
    same(before[head], after[head])
  )
    head++;
  let tail = 0;
  while (
    tail < before.length - head &&
    tail < after.length - head &&
    same(before[before.length - 1 - tail], after[after.length - 1 - tail])
  )
    tail++;

  const n = before.length - head - tail;
  const m = after.length - head - tail;
  if ((n + 1) * (m + 1) > maxCells) return null;

  // lcs[i][j]: longest common subsequence of before[head+i..] and after[head+j..].
  const width = m + 1;
  const lcs = new Uint32Array((n + 1) * width);
  for (let i = n - 1; i >= 0; i--)
    for (let j = m - 1; j >= 0; j--)
      lcs[i * width + j] = same(before[head + i], after[head + j])
        ? lcs[(i + 1) * width + j + 1] + 1
        : Math.max(lcs[(i + 1) * width + j], lcs[i * width + j + 1]);

  const ops: Op[] = [];
  for (let k = 0; k < head; k++)
    ops.push({ kind: "same", before: k, after: k });
  let i = 0;
  let j = 0;
  while (i < n || j < m) {
    if (i < n && j < m && same(before[head + i], after[head + j])) {
      ops.push({ kind: "same", before: head + i++, after: head + j++ });
    } else if (
      j === m ||
      // On a tie, removals come before the additions that replace them.
      (i < n && lcs[(i + 1) * width + j] >= lcs[i * width + j + 1])
    ) {
      ops.push({ kind: "removed", before: head + i++ });
    } else {
      ops.push({ kind: "added", after: head + j++ });
    }
  }
  for (let k = 0; k < tail; k++)
    ops.push({
      kind: "same",
      before: before.length - tail + k,
      after: after.length - tail + k,
    });
  return ops;
}

/**
 * The changed lines between `before` and `after`, grouped into hunks with
 * `context` unchanged lines around each change. Identical text has no hunks.
 * A line whose only change is its ending is reported as changed: the preview
 * never hides a difference the file will have.
 */
export function diffLines(
  before: string,
  after: string,
  {
    context = 3,
    maxCells = DEFAULT_MAX_CELLS,
  }: { context?: number; maxCells?: number } = {},
): LineDiff {
  const a = splitLines(before);
  const b = splitLines(after);
  const ops = lineOps(a, b, maxCells);
  if (!ops) return { tooLarge: true };

  const lines: DiffLine[] = ops.map((op) => {
    const line = op.before !== undefined ? a[op.before] : b[op.after!];
    return {
      kind: op.kind,
      text: line.text,
      ending: line.ending,
      ...(op.before !== undefined ? { beforeLine: op.before + 1 } : {}),
      ...(op.after !== undefined ? { afterLine: op.after + 1 } : {}),
    };
  });

  // Keep each changed line and its context; merge hunks whose context touches.
  const keep = new Array<boolean>(lines.length).fill(false);
  lines.forEach((line, index) => {
    if (line.kind === "same") return;
    const from = Math.max(0, index - context);
    const to = Math.min(lines.length - 1, index + context);
    for (let k = from; k <= to; k++) keep[k] = true;
  });
  const hunks: DiffHunk[] = [];
  let current: DiffLine[] | null = null;
  lines.forEach((line, index) => {
    if (!keep[index]) {
      current = null;
      return;
    }
    if (!current) {
      current = [];
      hunks.push({ lines: current });
    }
    current.push(line);
  });
  return { tooLarge: false, hunks };
}

/**
 * A textarea reports every line break as `\n`, whatever the file uses. Put the
 * file's own endings back so an edit doesn't silently rewrite every line:
 * unchanged lines keep the ending they had, and new lines take the file's
 * most common ending. Unedited text comes back exactly as the original.
 */
export function restoreLineEndings(
  original: string,
  edited: string,
  maxCells = DEFAULT_MAX_CELLS,
): string {
  if (!original.includes("\r")) return edited;
  if (original.replace(/\r\n?/g, "\n") === edited) return original;

  const before = splitLines(original);
  const counts = { "\r\n": 0, "\n": 0, "\r": 0 };
  for (const line of before) if (line.ending) counts[line.ending]++;
  const common: LineEnding =
    counts["\r\n"] >= counts["\n"] && counts["\r\n"] >= counts["\r"]
      ? "\r\n"
      : counts["\n"] >= counts["\r"]
        ? "\n"
        : "\r";

  const after = splitLines(edited);
  // Compare text only: the edited lines' endings are the textarea's, not the file's.
  const ops = lineOps(before, after, maxCells, (x, y) => x.text === y.text);
  const endingOf = new Array<LineEnding | undefined>(after.length);
  for (const op of ops ?? [])
    if (op.kind === "same") endingOf[op.after!] = before[op.before!].ending;

  return after
    .map((line, index) => {
      // The edited text decides whether there is a line break; the file decides which.
      if (!line.ending) return line.text;
      const kept = endingOf[index];
      return line.text + (kept ? kept : common);
    })
    .join("");
}

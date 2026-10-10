/**
 * Search operators, in the style of Google Scholar. A query that uses none
 * keeps the plain search; one that does is matched exactly. The twin of
 * `src-tauri/src/search_query.rs`, which matches indexed text; this one
 * matches names (and loaded text) before a folder is indexed. Change both
 * together.
 */

const MAX_TERMS = 32;
const FIELDS = new Set(["intitle", "filetype", "ext", "in", "folder"]);

export interface SearchQuery {
  /** Every clause must match; a clause matches when any of its terms does. */
  clauses: string[][];
  excluded: string[];
  inTitle: string[];
  filetypes: string[];
  folders: string[];
}

/** The parts of a file a query is matched against. */
export interface Searchable {
  name: string;
  title?: string;
  relativePath: string;
  text?: string;
}

/** What the cheatsheet lists, in order. */
export const SEARCH_OPERATORS: readonly {
  example: string;
  meaning: string;
}[] = [
  {
    example: '"1_b"',
    meaning: "Exact text only: not 1_c, 1 b or 2_b",
  },
  { example: "budget -draft", meaning: "Leave out files with a word" },
  {
    example: "budget OR gastos",
    meaning: "Either word (OR in capitals)",
  },
  { example: "intitle:resume", meaning: "In the file's name" },
  { example: "filetype:pdf", meaning: "Only this file type" },
  { example: "in:projects", meaning: "Only inside a folder" },
];

/** Case- and accent-insensitive, with any run of whitespace as one space. */
export function foldText(value: string): string {
  return value
    .normalize("NFD")
    .replace(/\p{M}/gu, "")
    .toLocaleLowerCase()
    .split(/\s+/u)
    .filter(Boolean)
    .join(" ");
}

const isWord = (character: string | undefined) =>
  character !== undefined && /[\p{L}\p{N}_]/u.test(character);

/** How often `needle` occurs in `haystack` as a whole token (both folded). */
function occurrences(haystack: string, needle: string): number {
  if (!needle) return 0;
  let count = 0;
  for (
    let at = haystack.indexOf(needle);
    at !== -1;
    at = haystack.indexOf(needle, at + 1)
  ) {
    const before = [...haystack.slice(0, at)].at(-1);
    const after = [...haystack.slice(at + needle.length)][0];
    if (!isWord(before) && !isWord(after)) count++;
  }
  return count;
}

interface Token {
  negated: boolean;
  field?: string;
  value: string;
  quoted: boolean;
}

function tokens(query: string): Token[] {
  const found: Token[] = [];
  const characters = [...query];
  let index = 0;
  while (index < characters.length) {
    if (/\s/u.test(characters[index])) {
      index++;
      continue;
    }
    const negated = characters[index] === "-";
    if (negated) index++;
    let raw = "";
    let quoted = false;
    let field: string | undefined;
    while (index < characters.length) {
      const character = characters[index];
      if (character === '"') {
        quoted = true;
        index++;
        while (index < characters.length && characters[index] !== '"')
          raw += characters[index++];
        index++;
        break;
      }
      if (/\s/u.test(character)) break;
      raw += character;
      index++;
      if (character === ":" && field === undefined) {
        const name = raw.slice(0, -1).toLocaleLowerCase();
        if (FIELDS.has(name)) {
          field = name;
          raw = "";
        }
      }
    }
    if (negated && !raw && !quoted && field === undefined) continue;
    found.push({ negated, field, value: raw, quoted });
  }
  return found;
}

/** `null` when the query uses no operator, so it keeps the plain search. */
export function parseSearchQuery(query: string): SearchQuery | null {
  const all = tokens(query);
  const usesOperators = all.some(
    (token) =>
      token.negated ||
      token.quoted ||
      token.field !== undefined ||
      token.value === "OR" ||
      token.value === "AND",
  );
  if (!usesOperators) return null;
  const parsed: SearchQuery = {
    clauses: [],
    excluded: [],
    inTitle: [],
    filetypes: [],
    folders: [],
  };
  let joinNext = false;
  let terms = 0;
  for (const token of all) {
    if (!token.quoted && !token.negated && token.field === undefined) {
      if (token.value === "OR") {
        joinNext = parsed.clauses.length > 0;
        continue;
      }
      if (token.value === "AND") continue;
    }
    const value = foldText(token.value);
    if (!value || terms >= MAX_TERMS) continue;
    terms++;
    if (token.negated) parsed.excluded.push(value);
    else if (token.field === "intitle") parsed.inTitle.push(value);
    else if (token.field === "filetype" || token.field === "ext")
      parsed.filetypes.push(value.replace(/^\.+/u, ""));
    else if (token.field === "in" || token.field === "folder")
      parsed.folders.push(
        value.replace(/\\/gu, "/").replace(/^\/+|\/+$/gu, ""),
      );
    else if (joinNext) parsed.clauses.at(-1)!.push(value);
    else parsed.clauses.push([value]);
    joinNext = false;
  }
  return parsed;
}

/** Whether a query uses search operators. */
export function hasSearchOperators(query: string): boolean {
  return parseSearchQuery(query) !== null;
}

/** The terms a match should be highlighted by. */
export function highlightTerms(parsed: SearchQuery): string[] {
  return [...parsed.clauses.flat(), ...parsed.inTitle];
}

/** How strongly a file matches, or `null` when it doesn't. */
export function scoreFile(
  parsed: SearchQuery,
  file: Searchable,
): number | null {
  const extension = file.name.includes(".")
    ? file.name.split(".").at(-1)!.toLocaleLowerCase()
    : "";
  if (parsed.filetypes.length && !parsed.filetypes.includes(extension))
    return null;
  if (parsed.folders.length) {
    const slash = file.relativePath.lastIndexOf("/");
    const folder = `/${foldText(slash === -1 ? "" : file.relativePath.slice(0, slash))}/`;
    if (!parsed.folders.some((wanted) => folder.includes(`/${wanted}/`)))
      return null;
  }
  const named = foldText(`${file.name} ${file.title ?? ""}`);
  // File names use `_` as a separator ("VILAR_Resume"), so a name is also
  // read with it as a space. A term with `_` in it still needs it.
  const title = `${named} ${named.replace(/_/gu, " ")}`;
  const body = foldText(file.text ?? "");
  let score = 0;
  for (const wanted of parsed.inTitle) {
    const found = occurrences(title, wanted);
    if (!found) return null;
    score += 5 + found;
  }
  for (const clause of parsed.clauses) {
    let matched = false;
    for (const term of clause) {
      const inTitle = occurrences(title, term);
      const inBody = occurrences(body, term);
      if (inTitle + inBody > 0) {
        matched = true;
        score += 3 * Math.min(inTitle, 1) + Math.min(inBody, 20);
      }
    }
    if (!matched) return null;
  }
  if (
    parsed.excluded.some(
      (term) => occurrences(title, term) + occurrences(body, term) > 0,
    )
  )
    return null;
  return score;
}

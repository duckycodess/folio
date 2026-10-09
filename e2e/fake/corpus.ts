import { createHash } from "node:crypto";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";
import type { MediaType } from "../../src/domain/contracts";
import type { FakeFileInput, FakePageRange } from "./types";

const DOCUMENTS = fileURLToPath(
  new URL("../../fixtures/documents", import.meta.url),
);

function walk(directory: string): string[] {
  const found: string[] = [];
  for (const entry of readdirSync(directory).sort()) {
    const path = join(directory, entry);
    if (statSync(path).isDirectory()) found.push(...walk(path));
    else found.push(path);
  }
  return found;
}

function mediaTypeFor(relativePath: string): MediaType | undefined {
  if (relativePath.endsWith(".md") || relativePath.endsWith(".markdown"))
    return "text/markdown";
  if (relativePath.endsWith(".txt")) return "text/plain";
  if (relativePath.endsWith(".pdf")) return "application/pdf";
  return undefined;
}

/**
 * The text of an uncompressed text-layer PDF, page by page.
 *
 * The in-repo fixture PDFs are written by `src-tauri`'s own generator with one
 * uncompressed content stream per page and one `(text) Tj` per line, so the
 * text the journeys see comes from the fixture bytes rather than from a copy of
 * it kept beside them. A PDF this cannot read contributes no text, and the fake
 * then reports it as `unsupported`, which is what a scanned PDF is.
 */
function pdfPages(bytes: Buffer): string[] {
  const raw = bytes.toString("latin1");
  const objects = new Map<string, string>();
  for (const match of raw.matchAll(/(\d+) 0 obj([\s\S]*?)endobj/g))
    objects.set(match[1], match[2]);
  const kids = raw.match(/\/Kids\s*\[([^\]]*)\]/);
  const order = kids
    ? Array.from(kids[1].matchAll(/(\d+) 0 R/g)).map((match) => match[1])
    : [];
  const pages: string[] = [];
  for (const id of order) {
    const page = objects.get(id);
    const contents = page?.match(/\/Contents (\d+) 0 R/);
    const stream = contents
      ? objects.get(contents[1])?.match(/stream\r?\n([\s\S]*?)\r?\nendstream/)
      : null;
    if (!stream) {
      pages.push("");
      continue;
    }
    const lines = Array.from(stream[1].matchAll(/\((.*?)\)\s*Tj/g)).map(
      (match) =>
        Buffer.from(match[1].replace(/\\([()\\])/g, "$1"), "latin1").toString(
          "utf8",
        ),
    );
    pages.push(lines.join("\n"));
  }
  return pages;
}

/**
 * The in-repo synthetic corpus, as the fake native core's starting workspace.
 * It is the same `fixtures/documents` tree the browser preview and the native
 * tests use, read from disk rather than restated here.
 */
export function fixtureCorpus(): FakeFileInput[] {
  const files: FakeFileInput[] = [];
  for (const path of walk(DOCUMENTS)) {
    const relativePath = relative(DOCUMENTS, path).split(sep).join("/");
    const mediaType = mediaTypeFor(relativePath);
    if (!mediaType) continue;
    const bytes = readFileSync(path);
    // Fixture modification times are fixed, so a table of dates is stable.
    const modifiedAtMs = Date.UTC(2026, 9, 1 + files.length, 9, 30);
    if (mediaType !== "application/pdf") {
      files.push({
        relativePath,
        content: bytes.toString("utf8"),
        mediaType,
        modifiedAtMs,
      });
      continue;
    }
    const pages = pdfPages(bytes);
    const ranges: FakePageRange[] = [];
    let content = "";
    for (const [position, page] of pages.entries()) {
      const text = page.replace(/\s+$/, "");
      if (position > 0) content += "\n\n";
      ranges.push({
        page: position + 1,
        startIndex: content.length,
        endIndex: content.length + text.length,
      });
      content += text;
    }
    files.push({
      relativePath,
      content,
      mediaType,
      modifiedAtMs,
      // A PDF's size and hash are the file's own, never the extracted text's.
      fileSizeBytes: bytes.byteLength,
      fileContentHash: `sha256:${createHash("sha256").update(bytes).digest("hex")}`,
      pages: ranges,
    });
  }
  return files;
}

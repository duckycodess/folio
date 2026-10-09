import type {
  ActionPlan,
  ApplyReport,
  Approval,
  BatchResult,
  BatchStopReason,
  ContentHash,
  DocumentId,
  DuplicateGroup,
  ExplicitReference,
  FileOperation,
  FolioErrorCode,
  FolioErrorDetails,
  HistoryEntry,
  ImpactCandidate,
  IndexProgress,
  IndexedDocument,
  MediaType,
  ModelDescriptor,
  ModelInstallState,
  ModelSetup,
  OperationOutcome,
  OperationStatus,
  OrganizationSuggestion,
  OrganizationSuggestions,
  ProviderIndexStatus,
  RelativePath,
  RuntimeStatus,
  ScanSummary,
  SearchResult,
  SourcePassage,
  UndoConflict,
  UndoPreflight,
  UndoReport,
  WorkspaceInfo,
} from "../../src/domain/contracts";
import type {
  FakeControl,
  FakeFailure,
  FakeFileInput,
  FakeNativeOptions,
  FakePageRange,
  FakeSkippedEntry,
  FakeWriterBehaviour,
} from "./types";

/** What the fake installs on the page, in place of the Tauri webview globals. */
export interface FakeScope {
  isTauri?: boolean;
  __TAURI_INTERNALS__?: {
    invoke(command: string, args?: Record<string, unknown>): Promise<unknown>;
    transformCallback(
      callback: (payload: unknown) => void,
      once?: boolean,
    ): number;
    unregisterCallback(id: number): void;
  };
  __TAURI_EVENT_PLUGIN_INTERNALS__?: {
    unregisterListener(event: string, eventId: number): void;
  };
  __folioFake?: FakeControl;
}

/**
 * Install a stateful stand-in for Folio's native core on this page.
 *
 * It is one self-contained function on purpose: Playwright serializes it into
 * the page with `addInitScript`, so it may not reference anything outside its
 * own body. The commands, their arguments and their replies are the ones in
 * `docs/contracts.md` and `src/adapters/`; hashes, UTF-8 source offsets, plan
 * digests, approvals, preflight refusals, durable outcomes and Undo conflicts
 * are computed here rather than canned, so a journey that passes against it has
 * exercised the real protocol.
 *
 * What it is not: evidence about the real Rust core, the Tauri webview, a real
 * filesystem, local inference, or any platform. It is a browser test double.
 */
export function installFakeNativeCore(options: FakeNativeOptions): void {
  // Native arguments and replies cross a JSON boundary. Keep page objects
  // separate from native state, including the test controls' own inputs.
  function wireCopy<T>(value: T): T {
    return value === undefined
      ? value
      : (JSON.parse(JSON.stringify(value)) as T);
  }

  options = wireCopy(options);
  /** Grepped for by `scripts/assert-production-excludes-fake.mjs`. */
  const SENTINEL = "folio-e2e-fake-native-core";
  const INDEX_PROGRESS_EVENT = "folio://index-progress";
  const MAX_SLUG_CHARS = 60;
  const MAX_IMPACT_CANDIDATES = 25;
  const MAX_PASSAGES_PER_RESULT = 3;

  const scope = globalThis as unknown as FakeScope;
  const encoder = new TextEncoder();
  const decoder = new TextDecoder("utf-8", { fatal: true });

  /* ------------------------------------------------------------- utilities */

  function fail(
    code: FolioErrorCode,
    message: string,
    details?: FolioErrorDetails,
  ): never {
    // The wire shape a native `Result::Err` serializes: never an `Error`.
    throw details
      ? { code, message, details: wireCopy(details) }
      : { code, message };
  }

  async function sha256(bytes: Uint8Array): Promise<ContentHash> {
    const digest = await crypto.subtle.digest(
      "SHA-256",
      bytes.slice().buffer as ArrayBuffer,
    );
    const hex = Array.from(new Uint8Array(digest))
      .map((byte) => byte.toString(16).padStart(2, "0"))
      .join("");
    return `sha256:${hex}`;
  }

  function utf8Length(text: string): number {
    return encoder.encode(text).length;
  }

  function utf8Offset(text: string, index: number): number {
    return encoder.encode(text.slice(0, index)).length;
  }

  function sleep(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
  }

  function nameOf(path: string): string {
    return path.split("/").slice(-1)[0] ?? path;
  }

  function folderOf(path: string): string {
    return path.split("/").slice(0, -1).join("/");
  }

  function mediaTypeForPath(path: string): MediaType | undefined {
    const name = nameOf(path).toLowerCase();
    if (name.endsWith(".md") || name.endsWith(".markdown"))
      return "text/markdown";
    if (name.endsWith(".txt")) return "text/plain";
    if (name.endsWith(".pdf")) return "application/pdf";
    return undefined;
  }

  /** The same `/`-separated, NFC-normalized relative path rule as the boundary. */
  function normalizeRelativePath(raw: string): RelativePath {
    if (typeof raw !== "string" || raw.length === 0)
      fail("pathNotRelative", "A relative document path is required.");
    const value = raw.normalize("NFC");
    if (/[\u0000-\u001f\u007f]/.test(value))
      fail(
        "pathNotRelative",
        "A document path cannot contain control characters.",
      );
    if (value.includes("\\"))
      fail("pathNotRelative", "Use '/' to separate path segments.", {
        path: raw,
      });
    if (value.startsWith("/") || /^[A-Za-z]:/.test(value))
      fail("pathNotRelative", "An absolute path cannot identify a document.", {
        path: raw,
      });
    for (const segment of value.split("/")) {
      if (segment.length === 0)
        fail(
          "pathNotRelative",
          "A document path cannot contain an empty segment.",
          {
            path: raw,
          },
        );
      if (segment === "." || segment === "..")
        fail(
          "pathEscapesWorkspace",
          "A document path cannot navigate outside the authorized folder.",
          { path: raw },
        );
    }
    return value;
  }

  /** Names Windows cannot store are refused for a path Folio would create. */
  function assertPortableDestination(raw: string): RelativePath {
    const path = normalizeRelativePath(raw);
    for (const segment of path.split("/"))
      if (/[<>:"|?*\\]/.test(segment) || /[ .]$/.test(segment))
        fail(
          "operationUnsupported",
          `'${segment}' cannot be stored on every supported platform. Choose another name.`,
          { path },
        );
    return path;
  }

  function documentIdFor(relativePath: string): DocumentId {
    return `${options.workspaceId}:${relativePath}`;
  }

  function titleOf(content: string, name: string): string {
    const heading = content.match(/^# (.+)$/m);
    return heading ? heading[1] : name;
  }

  /** Lowercased, accent-folded word characters: the index's query terms. */
  function terms(query: string): string[] {
    const folded = query
      .normalize("NFKC")
      .normalize("NFD")
      .replace(/\p{M}/gu, "")
      .toLocaleLowerCase();
    return Array.from(new Set(folded.match(/[\p{L}\p{N}]+/gu) ?? []));
  }

  /** Folds without changing length, so an offset in the folded text still fits. */
  function fold(text: string): string {
    return Array.from(text)
      .map((character) => {
        const lower = character.toLocaleLowerCase();
        return lower.length === character.length ? lower : character;
      })
      .join("");
  }

  function slugify(title: string): string {
    let slug = "";
    for (const character of title.toLocaleLowerCase()) {
      if (/[\p{L}\p{N}]/u.test(character)) slug += character;
      else if (slug.length > 0 && !slug.endsWith("-")) slug += "-";
    }
    return slug.replace(/-+$/, "");
  }

  /* ----------------------------------------------------------------- state */

  interface Stored {
    content: string;
    mediaType: MediaType;
    modifiedAtMs: number | null;
    /** PDFs only: the file's own size and hash, and its extracted pages. */
    fileSizeBytes?: number;
    fileContentHash?: ContentHash;
    pages?: FakePageRange[];
  }

  interface Indexed {
    contentHash: ContentHash;
    status: IndexedDocument["status"];
    statusMessage?: string;
    indexedAtMs: number;
  }

  interface StoredPlan {
    plan: ActionPlan;
    approval: Approval | null;
    state: "prepared" | "applied";
  }

  interface StoredHistory extends HistoryEntry {
    /** The text Undo would put back. Absent for a `create`. */
    beforeContent?: string;
  }

  const files = new Map<RelativePath, Stored>();
  for (const file of options.files) {
    const entry: Stored = {
      content: file.content,
      mediaType: file.mediaType,
      modifiedAtMs: file.modifiedAtMs,
    };
    if (file.fileSizeBytes !== undefined)
      entry.fileSizeBytes = file.fileSizeBytes;
    if (file.fileContentHash !== undefined)
      entry.fileContentHash = file.fileContentHash;
    if (file.pages !== undefined) entry.pages = file.pages;
    files.set(file.relativePath, entry);
  }

  const index = new Map<RelativePath, Indexed>();
  const plans = new Map<string, StoredPlan>();
  const history: StoredHistory[] = [];
  const listeners = new Map<string, Map<number, (payload: unknown) => void>>();
  const callbacks = new Map<number, (payload: unknown) => void>();
  const invoked: string[] = [];
  const failOnce = new Map<string, FakeFailure>();
  const failAlways = new Map<string, FakeFailure>();

  let authorized = false;
  let skipped: FakeSkippedEntry[] = options.skipped;
  let scanStepMs = options.scanStepMs;
  let writer: FakeWriterBehaviour = {};
  let cancelIndexing = false;
  let scanning: Promise<unknown> = Promise.resolve();
  let nextCallbackId = 1;
  let nextPlanNumber = 1;
  let nextHistoryNumber = 1;
  let lastScan: ScanSummary | null = null;
  /** Resolves once a pre-indexed folder is in the index; every command waits. */
  let ready: Promise<void> = Promise.resolve();

  function sizeOf(entry: Stored): number {
    return entry.fileSizeBytes ?? utf8Length(entry.content);
  }

  async function hashOf(entry: Stored): Promise<ContentHash> {
    return (
      entry.fileContentHash ?? (await sha256(encoder.encode(entry.content)))
    );
  }

  function fileAt(path: RelativePath): Stored {
    const entry = files.get(path);
    if (!entry)
      fail("documentUnavailable", "That file is no longer in the folder.", {
        path,
      });
    return entry;
  }

  function assertWorkspace(workspaceId: string): void {
    if (!authorized || workspaceId !== options.workspaceId)
      fail(
        "workspaceNotAuthorized",
        "Folio no longer has access to this folder.",
        { workspaceId: String(workspaceId) },
      );
  }

  /* ---------------------------------------------------------------- events */

  function emitProgress(progress: IndexProgress): void {
    emit(INDEX_PROGRESS_EVENT, progress);
  }

  function emit(event: string, payload: unknown): void {
    const handlers = listeners.get(event);
    if (!handlers) return;
    for (const [id, handler] of handlers)
      handler({ event, id, payload: wireCopy(payload) });
  }

  /* ------------------------------------------------------- the local index */

  async function indexDocument(path: RelativePath): Promise<void> {
    const entry = fileAt(path);
    const unsupported =
      entry.mediaType === "application/pdf" && !entry.content.trim();
    index.set(path, {
      contentHash: await hashOf(entry),
      status: unsupported ? "unsupported" : "indexed",
      ...(unsupported
        ? { statusMessage: "This PDF has no text layer Folio could read." }
        : {}),
      indexedAtMs: Date.now(),
    });
  }

  async function indexedRecord(path: RelativePath): Promise<IndexedDocument> {
    const entry = fileAt(path);
    const held = index.get(path);
    if (!held)
      fail("documentUnavailable", "That file is not in the index.", { path });
    const record: IndexedDocument = {
      id: documentIdFor(path),
      workspaceId: options.workspaceId,
      relativePath: path,
      name: nameOf(path),
      title: titleOf(entry.content, nameOf(path)),
      // The native index does not guess a language; detection is the provider's.
      language: "unknown",
      mediaType: entry.mediaType,
      sizeBytes: sizeOf(entry),
      contentHash: held.contentHash,
      status: held.status,
      indexedAtMs: held.indexedAtMs,
    };
    if (entry.modifiedAtMs !== null) record.modifiedAtMs = entry.modifiedAtMs;
    if (held.statusMessage !== undefined)
      record.statusMessage = held.statusMessage;
    return record;
  }

  function indexedPaths(): RelativePath[] {
    return Array.from(index.keys()).sort();
  }

  /** Paragraph chunks, as byte ranges: the unit search evidence is located in. */
  function chunksOf(path: RelativePath): {
    text: string;
    startIndex: number;
    page?: number;
  }[] {
    const entry = fileAt(path);
    const chunks: { text: string; startIndex: number; page?: number }[] = [];
    const pages = entry.pages ?? [
      { page: 0, startIndex: 0, endIndex: entry.content.length },
    ];
    for (const span of pages) {
      const segment = entry.content.slice(span.startIndex, span.endIndex);
      let at = 0;
      for (const paragraph of segment.split(/\n{2,}/)) {
        const startIndex = span.startIndex + segment.indexOf(paragraph, at);
        at = startIndex - span.startIndex + paragraph.length;
        if (!paragraph.trim()) continue;
        chunks.push({
          text: paragraph,
          startIndex,
          ...(span.page > 0 ? { page: span.page } : {}),
        });
      }
    }
    return chunks;
  }

  function passageOf(
    path: RelativePath,
    contentHash: ContentHash,
    chunk: { text: string; startIndex: number; page?: number },
  ): SourcePassage {
    const entry = fileAt(path);
    const start = utf8Offset(entry.content, chunk.startIndex);
    const passage: SourcePassage = {
      documentId: documentIdFor(path),
      documentContentHash: contentHash,
      offsetUnit: "utf8Byte",
      start,
      end: start + utf8Length(chunk.text),
      text: chunk.text,
    };
    if (chunk.page !== undefined) passage.page = chunk.page;
    return passage;
  }

  /* -------------------------------------------------------- relationships */

  /** A workspace-relative Markdown link target, or `undefined` when it leaves. */
  function linkedPath(sourcePath: string, link: string): string | undefined {
    if (/^(?:[a-z]+:|\/|\\)/i.test(link)) return undefined;
    let decoded: string;
    try {
      decoded = decodeURIComponent(link.split(/[?#]/)[0]);
    } catch {
      return undefined;
    }
    if (!decoded || /^(?:[a-z]+:|\/|\\)/i.test(decoded)) return undefined;
    const parts = sourcePath.split("/").slice(0, -1);
    for (const part of decoded.split("/")) {
      if (!part || part === ".") continue;
      if (part === "..") {
        if (!parts.length) return undefined;
        parts.pop();
      } else parts.push(part);
    }
    const joined = parts.join("/");
    return joined.length > 0 ? joined.normalize("NFC") : undefined;
  }

  async function explicitReferences(): Promise<ExplicitReference[]> {
    const found: ExplicitReference[] = [];
    for (const path of indexedPaths()) {
      const entry = fileAt(path);
      const sourceHash = index.get(path)!.contentHash;
      for (const match of entry.content.matchAll(/\[[^\]]+\]\(([^)]+)\)/g)) {
        const target = linkedPath(path, match[1]);
        if (!target || target === path) continue;
        const targetIndex = index.get(target);
        if (!targetIndex || !files.has(target)) continue;
        const startIndex = match.index ?? 0;
        const start = utf8Offset(entry.content, startIndex);
        found.push({
          type: "explicitReference",
          provenance: "documentLink",
          sourceId: documentIdFor(path),
          targetId: documentIdFor(target),
          sourceContentHash: sourceHash,
          targetContentHash: targetIndex.contentHash,
          link: { rawTarget: match[1], resolvedRelativePath: target },
          evidence: [
            {
              documentId: documentIdFor(path),
              documentContentHash: sourceHash,
              offsetUnit: "utf8Byte",
              start,
              end: start + utf8Length(match[0]),
              text: match[0],
            },
          ],
        });
      }
    }
    return found;
  }

  async function duplicateGroups(): Promise<DuplicateGroup[]> {
    const byHash = new Map<ContentHash, RelativePath[]>();
    for (const path of indexedPaths()) {
      const held = index.get(path)!;
      if (held.status !== "indexed" && held.status !== "unsupported") continue;
      const members = byHash.get(held.contentHash) ?? [];
      members.push(path);
      byHash.set(held.contentHash, members);
    }
    const groups: DuplicateGroup[] = [];
    for (const [contentHash, paths] of Array.from(byHash.entries()).sort(
      ([left], [right]) => left.localeCompare(right),
    )) {
      if (paths.length < 2) continue;
      // The stored hash only nominates a group; the bytes are compared again.
      const confirmed = paths.filter(
        (path) => fileAt(path).content === fileAt(paths[0]).content,
      );
      if (confirmed.length < 2) continue;
      const documents = await Promise.all(confirmed.map(indexedRecord));
      groups.push({
        contentHash,
        sizeBytes: documents[0].sizeBytes,
        documents,
      });
    }
    return groups;
  }

  /* ----------------------------------------------------- plans and digests */

  /** `FOLIO-PLAN-V1`, then every field length-prefixed in UTF-8 bytes. */
  function canonicalPlanBytes(plan: ActionPlan): Uint8Array {
    const field = (value: string) => `${utf8Length(value)}:${value}\n`;
    let text = "FOLIO-PLAN-V1\n";
    text += field(plan.id);
    text += field(plan.workspaceId);
    text += field(String(plan.createdAt));
    text += field(String(plan.expiresAt));
    text += field(String(plan.operations.length));
    for (const operation of plan.operations) {
      text += field(operation.kind);
      if (operation.kind === "create") {
        text += field(operation.destinationRelativePath);
        text += field(operation.mediaType);
        text += field(operation.content);
      } else if (operation.kind === "edit") {
        text += field(operation.documentId);
        text += field(operation.relativePath);
        text += field(operation.expectedContentHash);
        text += field(operation.after);
      } else {
        text += field(operation.documentId);
        text += field(operation.relativePath);
        text += field(operation.expectedContentHash);
        if (operation.kind !== "delete")
          text += field(operation.destinationRelativePath);
      }
    }
    return encoder.encode(text);
  }

  function isEditable(path: RelativePath): boolean {
    const mediaType = mediaTypeForPath(path);
    return mediaType === "text/plain" || mediaType === "text/markdown";
  }

  /**
   * The two-pass preflight of `docs/contracts.md`: the whole batch is checked
   * structurally first, then against the current files. Nothing is written.
   */
  async function preflight(plan: ActionPlan, now: number): Promise<void> {
    if (plan.operations.length === 0)
      fail("planEmpty", "This plan contains no operations.", {
        planId: plan.id,
      });
    if (now < plan.createdAt || now >= plan.expiresAt)
      fail("planExpired", "This preview is no longer current.", {
        planId: plan.id,
      });

    const touched = new Map<string, RelativePath>();
    const checked: { source?: RelativePath; destination?: RelativePath }[] = [];
    for (const operation of plan.operations) {
      let source: RelativePath | undefined;
      let destination: RelativePath | undefined;
      if (operation.kind !== "create") {
        source = normalizeRelativePath(operation.relativePath);
        if (!isEditable(source))
          fail(
            "unsupportedMediaType",
            "Folio edits TXT and Markdown files. Text-based PDFs are read-only.",
            { path: source },
          );
      }
      if (operation.kind !== "edit" && operation.kind !== "delete") {
        destination = assertPortableDestination(
          operation.destinationRelativePath,
        );
        if (!isEditable(destination))
          fail(
            "unsupportedMediaType",
            "Folio edits TXT and Markdown files. Text-based PDFs are read-only.",
            { path: destination },
          );
        if (
          source !== undefined &&
          destination.toLowerCase() === source.toLowerCase()
        )
          fail(
            "operationUnsupported",
            "A rename needs a destination different from the current name.",
            { path: source },
          );
      }
      for (const path of [source, destination]) {
        if (path === undefined) continue;
        const earlier = touched.get(path.toLowerCase());
        if (earlier !== undefined)
          fail(
            "duplicateOperationTarget",
            "Two operations in this plan act on the same file.",
            { path, earlierPath: earlier },
          );
        touched.set(path.toLowerCase(), path);
      }
      checked.push({ source, destination });
    }

    for (const [position, operation] of plan.operations.entries()) {
      const { source, destination } = checked[position];
      if (source !== undefined && operation.kind !== "create") {
        const current = files.get(source);
        if (!current)
          fail(
            "targetMissing",
            "The file this plan changes is no longer there.",
            {
              path: source,
            },
          );
        const observed = await hashOf(current);
        if (observed !== operation.expectedContentHash)
          fail(
            "targetChanged",
            "This file changed since the preview was prepared.",
            { path: source, expected: operation.expectedContentHash, observed },
          );
      }
      if (destination !== undefined && files.has(destination))
        fail(
          "destinationExists",
          "Something already uses that name. The existing file was left alone.",
          { path: destination },
        );
    }
  }

  /**
   * The phrase an edit replaces, widened to whole words and, for a bare day
   * number, to the month name before it. `undefined` for a pure insertion.
   */
  function replacedPhrase(before: string, after: string): string | undefined {
    let prefix = 0;
    while (
      prefix < before.length &&
      prefix < after.length &&
      before[prefix] === after[prefix]
    )
      prefix += 1;
    let suffix = 0;
    while (
      suffix < before.length - prefix &&
      suffix < after.length - prefix &&
      before[before.length - 1 - suffix] === after[after.length - 1 - suffix]
    )
      suffix += 1;
    let start = prefix;
    let end = before.length - suffix;
    if (start >= end) return undefined;
    while (start > 0 && /[\p{L}\p{N}]/u.test(before[start - 1])) start -= 1;
    while (end < before.length && /[\p{L}\p{N}]/u.test(before[end])) end += 1;
    if (/^\d+$/.test(before.slice(start, end))) {
      const lead = before.slice(0, start).replace(/ +$/, "");
      const word = lead.match(/[\p{L}]+$/u);
      if (word && lead.length < start) start = lead.length - word[0].length;
    }
    const phrase = before.slice(start, end).trim();
    return phrase.length > 0 && phrase.length <= 120 ? phrase : undefined;
  }

  /** Byte positions where `needle` occurs in `haystack` as a whole phrase. */
  function phrasePositions(haystack: string, needle: string): number[] {
    const positions: number[] = [];
    if (!needle) return positions;
    let at = haystack.indexOf(needle);
    while (at !== -1) {
      const before = haystack[at - 1];
      const after = haystack[at + needle.length];
      if (
        (before === undefined || !/[\p{L}\p{N}]/u.test(before)) &&
        (after === undefined || !/[\p{L}\p{N}]/u.test(after))
      )
        positions.push(at);
      at = haystack.indexOf(needle, at + 1);
    }
    return positions;
  }

  /**
   * Folio Ripple for one edit, as review candidates only.
   *
   * Like the native core it reports a neighbour that mentions the replaced
   * phrase: a document linked to or from the target is `evidence`, and a
   * byte-identical copy is `similarityOnly`. Unlike the native core it does not
   * look for the same date written with other month names, because no browser
   * journey reaches an edit yet; that is a limitation of this test double.
   */
  async function editImpacts(
    operation: Extract<FileOperation, { kind: "edit" }>,
    excluded: Set<DocumentId>,
  ): Promise<ImpactCandidate[]> {
    const current = files.get(operation.relativePath);
    if (!current) return [];
    const phrase = replacedPhrase(current.content, operation.after);
    if (!phrase) return [];
    const targetId = documentIdFor(operation.relativePath);
    const targetHash = index.get(operation.relativePath)?.contentHash;
    const linked = new Set<RelativePath>();
    for (const reference of await explicitReferences()) {
      if (reference.sourceId === targetId)
        linked.add(reference.link.resolvedRelativePath);
      if (reference.targetId === targetId)
        linked.add(reference.sourceId.slice(options.workspaceId.length + 1));
    }
    const candidates: ImpactCandidate[] = [];
    const folded = fold(phrase);
    for (const path of indexedPaths()) {
      const documentId = documentIdFor(path);
      if (documentId === targetId || excluded.has(documentId)) continue;
      const copy =
        targetHash !== undefined && index.get(path)!.contentHash === targetHash;
      if (!linked.has(path) && !copy) continue;
      const hash = index.get(path)!.contentHash;
      const evidence = chunksOf(path)
        .filter((chunk) => phrasePositions(fold(chunk.text), folded).length > 0)
        .map((chunk) => passageOf(path, hash, chunk));
      if (!evidence.length) continue;
      candidates.push({
        documentId,
        relativePath: path,
        reason: copy
          ? `Byte-identical copy of the target before this edit; this plan does not change it. It mentions “${phrase}”.`
          : `Linked to the target and mentions “${phrase}”.`,
        evidence,
        strength: copy ? "similarityOnly" : "evidence",
        ...(copy
          ? {}
          : {
              relationshipType: "explicitReference" as const,
              provenance: "documentLink" as const,
            }),
      });
    }
    candidates.sort(
      (left, right) =>
        Number(left.strength !== "evidence") -
          Number(right.strength !== "evidence") ||
        left.relativePath.localeCompare(right.relativePath),
    );
    return candidates.slice(0, MAX_IMPACT_CANDIDATES);
  }

  async function planImpacts(
    operations: FileOperation[],
  ): Promise<ImpactCandidate[]> {
    const targeted = new Set<DocumentId>(
      operations
        .filter((operation) => operation.kind !== "create")
        .map((operation) => operation.documentId),
    );
    const found: ImpactCandidate[] = [];
    for (const operation of operations) {
      if (operation.kind !== "edit") continue;
      for (const candidate of await editImpacts(operation, targeted))
        if (
          !found.some(
            (existing) => existing.documentId === candidate.documentId,
          )
        )
          found.push(candidate);
    }
    return found.slice(0, MAX_IMPACT_CANDIDATES);
  }

  /* ---------------------------------------------------------- the writer */

  /** Applies one operation to the fake workspace and records how to reverse it. */
  async function perform(
    operation: FileOperation,
    planId: string,
    operationIndex: number,
  ): Promise<StoredHistory> {
    const appliedAt = Date.now();
    const id = `history-${nextHistoryNumber++}`;
    if (operation.kind === "create") {
      const destination = operation.destinationRelativePath;
      files.set(destination, {
        content: operation.content,
        mediaType: operation.mediaType,
        modifiedAtMs: appliedAt,
      });
      return {
        id,
        planId,
        operationIndex,
        operationKind: operation.kind,
        appliedAt,
        documentId: documentIdFor(destination),
        afterRelativePath: destination,
        afterContentHash: await hashOf(fileAt(destination)),
        recoverable: true,
      };
    }
    const source = operation.relativePath;
    const entry = fileAt(source);
    const beforeContent = entry.content;
    const beforeContentHash = await hashOf(entry);
    if (operation.kind === "edit") {
      files.set(source, {
        ...entry,
        content: operation.after,
        modifiedAtMs: appliedAt,
      });
      return {
        id,
        planId,
        operationIndex,
        operationKind: operation.kind,
        appliedAt,
        documentId: operation.documentId,
        beforeRelativePath: source,
        afterRelativePath: source,
        beforeContentHash,
        afterContentHash: await hashOf(fileAt(source)),
        recoverable: true,
        beforeContent,
      };
    }
    if (operation.kind === "delete") {
      files.delete(source);
      return {
        id,
        planId,
        operationIndex,
        operationKind: operation.kind,
        appliedAt,
        documentId: operation.documentId,
        beforeRelativePath: source,
        beforeContentHash,
        beforeContent,
        recoverable: true,
      };
    }
    const destination = operation.destinationRelativePath;
    files.delete(source);
    files.set(destination, { ...entry, modifiedAtMs: appliedAt });
    return {
      id,
      planId,
      operationIndex,
      operationKind: operation.kind,
      appliedAt,
      documentId: operation.documentId,
      beforeRelativePath: source,
      afterRelativePath: destination,
      beforeContentHash,
      afterContentHash: beforeContentHash,
      recoverable: true,
      beforeContent,
    };
  }

  /** Re-reads the paths a batch touched, so the index matches the files again. */
  async function refreshIndexFor(paths: RelativePath[]): Promise<void> {
    for (const path of new Set(paths)) {
      if (files.has(path)) await indexDocument(path);
      else index.delete(path);
    }
  }

  function pathsOf(operation: FileOperation): RelativePath[] {
    if (operation.kind === "create") return [operation.destinationRelativePath];
    if (operation.kind === "edit" || operation.kind === "delete")
      return [operation.relativePath];
    return [operation.relativePath, operation.destinationRelativePath];
  }

  async function applyPlan(
    workspaceId: string,
    planId: string,
  ): Promise<ApplyReport> {
    assertWorkspace(workspaceId);
    const stored = plans.get(planId);
    if (!stored || stored.plan.workspaceId !== workspaceId)
      fail("planUnknown", "Folio has no preview with that identity.", {
        planId,
      });
    if (stored.state !== "prepared")
      fail("planStateInvalid", "This preview was already used or cancelled.", {
        planId,
      });
    const now = Date.now();
    if (now >= stored.plan.expiresAt)
      fail("planExpired", "This preview has expired.", { planId });
    if (!stored.approval)
      fail(
        "approvalRequired",
        "Approve this exact plan before any file changes.",
        {
          planId,
        },
      );
    if (stored.approval.planDigest !== stored.plan.digest)
      fail("approvalStale", "The plan changed after it was approved.", {
        planId,
      });
    // Every target is checked again before anything is written; a refusal here
    // means no file changed at all.
    await preflight(stored.plan, now);

    const startedAt = Date.now();
    const operations = stored.plan.operations;
    const outcomes: OperationOutcome[] = [];
    const touched: RelativePath[] = [];
    let stopReason: BatchStopReason = "completed";
    for (const [position, operation] of operations.entries()) {
      if (writer.failAtIndex === position) {
        const code = writer.failCode ?? "destinationExists";
        outcomes.push({
          operationIndex: position,
          status: "failed",
          completedAt: Date.now(),
          error: {
            code,
            message: "Folio could not finish this change, so it stopped here.",
            details: { path: pathsOf(operation).slice(-1)[0] },
          },
        });
        stopReason = "failed";
        break;
      }
      // Each operation re-checks its source immediately before it runs.
      if (operation.kind !== "create") {
        const current = files.get(operation.relativePath);
        const observed = current ? await hashOf(current) : undefined;
        if (observed !== operation.expectedContentHash) {
          outcomes.push({
            operationIndex: position,
            status: "failed",
            completedAt: Date.now(),
            error: {
              code: current ? "targetChanged" : "targetMissing",
              message: "This file changed after the preview, so Folio stopped.",
              details: { path: operation.relativePath },
            },
          });
          stopReason = "failed";
          break;
        }
      }
      const entry = await perform(operation, stored.plan.id, position);
      touched.push(...pathsOf(operation));
      if (writer.historyRequiredAtIndex === position) {
        // The file did change; only the record of how to reverse it is missing.
        outcomes.push({
          operationIndex: position,
          status: "failed",
          completedAt: Date.now(),
          error: {
            code: "historyRequired",
            message:
              "Folio saved the change but could not record how to reverse it.",
            details: {
              path: entry.afterRelativePath ?? entry.beforeRelativePath ?? "",
            },
          },
        });
        stopReason = "failed";
        break;
      }
      history.push(entry);
      outcomes.push({
        operationIndex: position,
        status: "succeeded",
        completedAt: entry.appliedAt,
        historyEntryId: entry.id,
      });
    }
    // No current screen can cancel a batch after saving began, so this fake
    // never produces `cancelled` outcomes; what it does produce is durable.
    const remaining: OperationStatus = "notStarted";
    for (
      let position = outcomes.length;
      position < operations.length;
      position += 1
    )
      outcomes.push({ operationIndex: position, status: remaining });

    stored.state = "applied";
    const indexRefreshed = writer.indexRefreshed ?? true;
    if (indexRefreshed) await refreshIndexFor(touched);
    const batch: BatchResult = {
      planId: stored.plan.id,
      planDigest: stored.plan.digest,
      startedAt,
      finishedAt: Date.now(),
      outcomes,
      stopReason,
    };
    return {
      batch,
      historySettled: writer.historySettled ?? true,
      indexRefreshed,
    };
  }

  /* ------------------------------------------------------------------ undo */

  function pendingEntries(planId: string): StoredHistory[] {
    return history.filter(
      (entry) => entry.planId === planId && entry.undoneAt === undefined,
    );
  }

  async function undoConflicts(
    entries: StoredHistory[],
  ): Promise<UndoConflict[]> {
    const conflicts: UndoConflict[] = [];
    for (const entry of entries) {
      const appliedPath = entry.afterRelativePath ?? entry.beforeRelativePath;
      if (!appliedPath || !entry.recoverable) {
        conflicts.push({
          historyEntryId: entry.id,
          ...(entry.documentId ? { documentId: entry.documentId } : {}),
          relativePath: appliedPath ?? "",
          observedContentHash: null,
          reason: "notRecoverable",
        });
        continue;
      }
      const current = files.get(appliedPath);
      if (entry.operationKind === "delete") {
        if (current)
          conflicts.push({
            historyEntryId: entry.id,
            documentId: entry.documentId,
            relativePath: appliedPath,
            observedContentHash: await hashOf(current),
            reason: "destinationOccupied",
          });
        continue;
      }
      if (!current) {
        conflicts.push({
          historyEntryId: entry.id,
          ...(entry.documentId ? { documentId: entry.documentId } : {}),
          relativePath: appliedPath,
          expectedContentHash: entry.afterContentHash,
          observedContentHash: null,
          reason: "missing",
        });
        continue;
      }
      const observed = await hashOf(current);
      if (observed !== entry.afterContentHash) {
        conflicts.push({
          historyEntryId: entry.id,
          ...(entry.documentId ? { documentId: entry.documentId } : {}),
          relativePath: appliedPath,
          expectedContentHash: entry.afterContentHash,
          observedContentHash: observed,
          reason: "externallyModified",
        });
        continue;
      }
      const restored = entry.beforeRelativePath;
      if (restored && restored !== appliedPath && files.has(restored))
        conflicts.push({
          historyEntryId: entry.id,
          ...(entry.documentId ? { documentId: entry.documentId } : {}),
          relativePath: restored,
          observedContentHash: await hashOf(fileAt(restored)),
          reason: "destinationOccupied",
        });
    }
    return conflicts;
  }

  async function previewUndo(
    workspaceId: string,
    planId: string,
  ): Promise<UndoPreflight> {
    assertWorkspace(workspaceId);
    const entries = pendingEntries(planId);
    const conflicts = await undoConflicts(entries);
    return {
      planId,
      entryIds: entries.map((entry) => entry.id),
      conflicts,
      undoable: conflicts.length === 0,
    };
  }

  async function undoPlan(
    workspaceId: string,
    planId: string,
    entryIds: string[],
  ): Promise<UndoReport> {
    assertWorkspace(workspaceId);
    const entries = pendingEntries(planId);
    if (!entries.length)
      fail(
        "planStateInvalid",
        "Everything in this change has already been undone.",
        { planId },
      );
    const shown = entryIds.slice().sort();
    const current = entries.map((entry) => entry.id).sort();
    if (
      shown.length !== current.length ||
      shown.some((id, at) => id !== current[at])
    )
      fail(
        "approvalStale",
        "What Undo would change is different from the preview you confirmed.",
        { planId },
      );
    const conflicts = await undoConflicts(entries);
    if (conflicts.length)
      fail(
        "undoConflict",
        "One of these files changed after Folio saved it, so nothing was undone.",
        {
          planId,
          blockingRelativePath: conflicts[0].relativePath,
          blockingHistoryEntryId: conflicts[0].historyEntryId,
          reason: conflicts[0].reason,
          conflicts: String(conflicts.length),
        },
      );

    const undone: string[] = [];
    const touched: RelativePath[] = [];
    const reversed = entries.slice().reverse();
    for (const entry of reversed) {
      if (
        writer.undoStopAfter !== undefined &&
        undone.length >= writer.undoStopAfter
      )
        break;
      const appliedPath = entry.afterRelativePath ?? entry.beforeRelativePath!;
      if (entry.operationKind === "delete") {
        files.set(appliedPath, {
          content: entry.beforeContent ?? "",
          mediaType: mediaTypeForPath(appliedPath)!,
          modifiedAtMs: Date.now(),
        });
      } else if (entry.beforeRelativePath === undefined) {
        files.delete(appliedPath);
      } else if (entry.beforeRelativePath === appliedPath) {
        files.set(appliedPath, {
          ...fileAt(appliedPath),
          content: entry.beforeContent ?? "",
          modifiedAtMs: Date.now(),
        });
      } else {
        const moved = fileAt(appliedPath);
        files.delete(appliedPath);
        files.set(entry.beforeRelativePath, {
          ...moved,
          modifiedAtMs: Date.now(),
        });
      }
      entry.undoneAt = Date.now();
      undone.push(entry.id);
      touched.push(appliedPath, entry.beforeRelativePath ?? appliedPath);
    }
    await refreshIndexFor(touched);
    const remainingEntryIds = entries
      .filter((entry) => entry.undoneAt === undefined)
      .map((entry) => entry.id);
    return {
      planId,
      undoneEntryIds: undone,
      remainingEntryIds,
      indexRefreshed: true,
    };
  }

  /* --------------------------------------------------------- keyword search */

  async function searchIndex(
    workspaceId: string,
    query: string,
    limit: number,
  ): Promise<SearchResult[]> {
    assertWorkspace(workspaceId);
    const wanted = terms(query);
    if (!wanted.length) return [];
    const found: SearchResult[] = [];
    for (const path of indexedPaths()) {
      const held = index.get(path)!;
      if (held.status !== "indexed") continue;
      const passages: SourcePassage[] = [];
      let matched = new Set<string>();
      for (const chunk of chunksOf(path)) {
        const folded = terms(chunk.text);
        const hits = wanted.filter((term) =>
          folded.some((word) => word === term || word.startsWith(term)),
        );
        if (!hits.length) continue;
        for (const hit of hits) matched.add(hit);
        if (passages.length < MAX_PASSAGES_PER_RESULT)
          passages.push(passageOf(path, held.contentHash, chunk));
      }
      if (!matched.size) continue;
      found.push({
        document: await indexedRecord(path),
        passages,
        score: matched.size / wanted.length,
        // Keyword matching, and labelled as such: this is not semantic retrieval.
        method: "keyword",
      });
    }
    found.sort(
      (left, right) =>
        right.score - left.score ||
        left.document.relativePath.localeCompare(right.document.relativePath),
    );
    return found.slice(0, Math.max(1, Math.min(100, limit)));
  }

  /* ------------------------------------------------------------ local sync */

  async function scanWorkspace(workspaceId: string): Promise<ScanSummary> {
    assertWorkspace(workspaceId);
    const startedAt = Date.now();
    cancelIndexing = false;
    const paths = Array.from(files.keys()).sort();
    let processed = 0;
    let added = 0;
    let updated = 0;
    let unchanged = 0;
    let unsupported = 0;
    emitProgress({
      workspaceId,
      phase: "discovering",
      processed: 0,
      total: paths.length,
    });
    let cancelled = false;
    for (const path of paths) {
      if (cancelIndexing) {
        cancelled = true;
        break;
      }
      await sleep(scanStepMs);
      // Stopping is checked again before a file is committed to the index, so
      // a scan stopped before its first file leaves the index as it was.
      if (cancelIndexing) {
        cancelled = true;
        break;
      }
      const before = index.get(path);
      const hash = await hashOf(fileAt(path));
      if (!before) added += 1;
      else if (before.contentHash !== hash) updated += 1;
      else unchanged += 1;
      await indexDocument(path);
      if (index.get(path)!.status === "unsupported") unsupported += 1;
      processed += 1;
      emitProgress({
        workspaceId,
        phase: "indexing",
        processed,
        total: paths.length,
        currentPath: path,
      });
    }
    let removed = 0;
    if (!cancelled) {
      for (const path of indexedPaths())
        if (!files.has(path)) {
          index.delete(path);
          removed += 1;
        }
      emitProgress({
        workspaceId,
        phase: "linking",
        processed,
        total: paths.length,
      });
    }
    emitProgress({
      workspaceId,
      phase: cancelled ? "cancelled" : "done",
      processed,
      total: paths.length,
    });
    // Stopping is not a failure: the summary says so and keeps what was indexed.
    lastScan = {
      workspaceId,
      total: index.size,
      added,
      updated,
      unchanged,
      removed,
      unsupported,
      failed: 0,
      stale: 0,
      deferred: 0,
      skipped: skipped.length,
      cancelled,
      durationMs: Date.now() - startedAt,
    };
    return lastScan;
  }

  /* ---------------------------------------------------- organize suggestions */

  async function organizationSuggestions(
    workspaceId: string,
  ): Promise<OrganizationSuggestions> {
    assertWorkspace(workspaceId);
    const taken = new Set(indexedPaths().map((path) => path.toLowerCase()));
    const filenames: OrganizationSuggestion[] = [];
    for (const path of indexedPaths()) {
      const held = index.get(path)!;
      const entry = fileAt(path);
      if (held.status !== "indexed" || entry.mediaType === "application/pdf")
        continue;
      const name = nameOf(path);
      const dot = name.lastIndexOf(".");
      if (dot <= 0) continue;
      const stem = name.slice(0, dot);
      const extension = name.slice(dot + 1);
      const title = titleOf(entry.content, name);
      const slug = slugify(title);
      if (!slug || Array.from(slug).length > MAX_SLUG_CHARS) continue;
      if (slug === stem.toLocaleLowerCase()) continue;
      const folder = folderOf(path);
      const suggested = folder
        ? `${folder}/${slug}.${extension}`
        : `${slug}.${extension}`;
      if (taken.has(suggested.toLowerCase()) || files.has(suggested)) continue;
      taken.add(suggested.toLowerCase());
      filenames.push({
        documentId: documentIdFor(path),
        relativePath: path,
        suggestedRelativePath: suggested,
        reason: `Named after the document's title “${title}”.`,
        operation: {
          kind: "rename",
          documentId: documentIdFor(path),
          relativePath: path,
          expectedContentHash: held.contentHash,
          destinationRelativePath: suggested,
          expectedDestination: "absent",
        },
      });
    }
    return { duplicateGroups: await duplicateGroups(), filenames };
  }

  /* --------------------------------------------------------- the commands */

  const commands: Record<
    string,
    (args: Record<string, unknown>) => Promise<unknown>
  > = {
    async choose_workspace(): Promise<WorkspaceInfo | null> {
      if (options.dismissFolderPicker) return null;
      authorized = true;
      return {
        id: options.workspaceId,
        rootPath: options.rootPath,
        authorizedAt: options.authorizedAt,
      };
    },

    async list_workspaces() {
      return [
        {
          id: options.workspaceId,
          rootPath: options.rootPath,
          authorizedAt: options.authorizedAt,
          lastOpenedAt: null,
          available: true,
        },
      ];
    },

    async reopen_workspace(args): Promise<WorkspaceInfo> {
      if (args.workspaceId !== options.workspaceId)
        fail("workspaceUnavailable", "That folder can't be reached.");
      authorized = true;
      return {
        id: options.workspaceId,
        rootPath: options.rootPath,
        authorizedAt: options.authorizedAt,
      };
    },

    async list_documents(args) {
      assertWorkspace(String(args.workspaceId));
      const documents = Array.from(files.keys())
        .sort()
        .map((path) => {
          const entry = fileAt(path);
          return {
            id: documentIdFor(path),
            workspaceId: options.workspaceId,
            relativePath: path,
            name: nameOf(path),
            mediaType: entry.mediaType,
            sizeBytes: sizeOf(entry),
            modifiedAtMs: entry.modifiedAtMs,
          };
        });
      return { workspaceId: options.workspaceId, documents, skipped };
    },

    async read_document(args) {
      assertWorkspace(String(args.workspaceId));
      const path = normalizeRelativePath(String(args.relativePath));
      const entry = fileAt(path);
      return {
        content: entry.content,
        contentHash: await hashOf(entry),
        sizeBytes: sizeOf(entry),
        modifiedAtMs: entry.modifiedAtMs,
        ...(entry.pages
          ? {
              pages: entry.pages.map((page) => ({
                page: page.page,
                start: utf8Offset(entry.content, page.startIndex),
                end: utf8Offset(entry.content, page.endIndex),
              })),
            }
          : {}),
      };
    },

    async list_indexed_documents(args) {
      assertWorkspace(String(args.workspaceId));
      return Promise.all(indexedPaths().map(indexedRecord));
    },

    async scan_workspace(args) {
      const workspaceId = String(args.workspaceId);
      assertWorkspace(workspaceId);
      const run = scanning.then(
        () => scanWorkspace(workspaceId),
        () => scanWorkspace(workspaceId),
      );
      scanning = run.catch(() => undefined);
      return run;
    },

    async recheck_documents(args) {
      assertWorkspace(String(args.workspaceId));
      const ids = (args.documentIds as string[]) ?? [];
      const paths = ids.map((id) => id.slice(options.workspaceId.length + 1));
      for (const path of paths) if (files.has(path)) await indexDocument(path);
      return Promise.all(
        paths.filter((path) => index.has(path)).map(indexedRecord),
      );
    },

    async cancel_indexing() {
      cancelIndexing = true;
      return undefined;
    },

    async search_index(args) {
      return searchIndex(
        String(args.workspaceId),
        String(args.query),
        Number(args.limit ?? 20),
      );
    },

    async list_models(): Promise<ModelDescriptor[]> {
      return options.models ?? [];
    },

    async model_setup(): Promise<ModelSetup> {
      return {
        selectedEmbedding: null,
        selectedGeneration: null,
        hostRuntimeId: options.runtime?.id ?? "e2e-runtime",
        hostRuntimeBytes: options.runtime?.bytes ?? null,
        deviceMemoryBytes: null,
        availableDiskBytes: null,
      };
    },

    async runtime_status(args): Promise<RuntimeStatus> {
      return {
        id: String(args.runtimeId),
        version: options.runtime?.version ?? "none",
        installed: false,
      };
    },

    async verify_model(args): Promise<ModelInstallState> {
      return { id: String(args.modelId), status: "notInstalled" };
    },

    async index_status(): Promise<ProviderIndexStatus> {
      return {
        ...(authorized ? { workspaceId: options.workspaceId } : {}),
        documentCount: index.size,
        chunkCount: index.size,
        method: "keyword",
      };
    },

    async rebuild_index(args): Promise<ProviderIndexStatus> {
      await scanWorkspace(String(args.workspaceId));
      return {
        workspaceId: options.workspaceId,
        documentCount: index.size,
        chunkCount: index.size,
        method: "keyword",
      };
    },

    async semantic_search(args) {
      return searchIndex(
        String(args.workspaceId),
        String(args.query),
        Number(args.limit ?? 10),
      );
    },

    async interpret_request() {
      fail("modelNotInstalled", "No local generation model is installed.");
    },

    async summarize_document() {
      fail("modelNotInstalled", "No local generation model is installed.");
    },

    async answer_question() {
      fail("modelNotInstalled", "No local generation model is installed.");
    },

    async list_relationships(args) {
      assertWorkspace(String(args.workspaceId));
      return explicitReferences();
    },

    async list_duplicates(args) {
      assertWorkspace(String(args.workspaceId));
      return duplicateGroups();
    },

    async organization_suggestions(args) {
      return organizationSuggestions(String(args.workspaceId));
    },

    async prepare_plan(args): Promise<ActionPlan> {
      const workspaceId = String(args.workspaceId);
      assertWorkspace(workspaceId);
      const operations = (args.operations as FileOperation[]) ?? [];
      const now = Date.now();
      const supplied = args.impacts as ImpactCandidate[] | undefined;
      const plan: ActionPlan = {
        id: `plan-${nextPlanNumber++}`,
        workspaceId,
        createdAt: now,
        expiresAt: now + options.planLifetimeMs,
        operations,
        impacts: supplied ?? (await planImpacts(operations)),
        digest: "",
      };
      plan.digest = await sha256(canonicalPlanBytes(plan));
      // Nothing is written: the plan is checked against the current files and
      // stored so an approval can be bound to it.
      await preflight(plan, now);
      plans.set(plan.id, { plan, approval: null, state: "prepared" });
      return plan;
    },

    async approve_plan(args): Promise<Approval> {
      const workspaceId = String(args.workspaceId);
      assertWorkspace(workspaceId);
      const planId = String(args.planId);
      const stored = plans.get(planId);
      if (!stored || stored.plan.workspaceId !== workspaceId)
        fail("planUnknown", "Folio has no preview with that identity.", {
          planId,
        });
      if (stored.state !== "prepared")
        fail(
          "planStateInvalid",
          "This preview was already used or cancelled.",
          {
            planId,
          },
        );
      if (Date.now() >= stored.plan.expiresAt)
        fail("planExpired", "This preview has expired.", { planId });
      if (String(args.planDigest) !== stored.plan.digest)
        fail("planDigestMismatch", "The approval does not match the preview.", {
          planId,
        });
      stored.approval = {
        planId,
        planDigest: stored.plan.digest,
        approvedAt: Date.now(),
      };
      return stored.approval;
    },

    async apply_plan(args) {
      return applyPlan(String(args.workspaceId), String(args.planId));
    },

    async cancel_apply() {
      return undefined;
    },

    async preview_undo(args) {
      return previewUndo(String(args.workspaceId), String(args.planId));
    },

    async undo_plan(args) {
      return undoPlan(
        String(args.workspaceId),
        String(args.planId),
        (args.entryIds as string[]) ?? [],
      );
    },

    async list_history(args) {
      assertWorkspace(String(args.workspaceId));
      const limit = Number(args.limit ?? 100);
      return history
        .slice()
        .reverse()
        .slice(0, limit)
        .map(({ beforeContent: _unused, ...entry }) => entry);
    },

    async ["plugin:event|listen"](args) {
      const event = String(args.event);
      const id = Number(args.handler);
      const handler = callbacks.get(id);
      if (!handler)
        fail("internal", "No handler was registered for that listener.");
      const handlers = listeners.get(event) ?? new Map();
      handlers.set(id, handler);
      listeners.set(event, handlers);
      return id;
    },

    async ["plugin:event|unlisten"](args) {
      const handlers = listeners.get(String(args.event));
      handlers?.delete(Number(args.eventId));
      callbacks.delete(Number(args.eventId));
      return undefined;
    },
  };

  /* ----------------------------------------------------------- the bridge */

  async function invoke(
    command: string,
    args: Record<string, unknown> = {},
  ): Promise<unknown> {
    // Capture the caller's values before yielding, as an IPC request does.
    const wireArgs = wireCopy(args);
    await ready;
    invoked.push(command);
    const injected = failOnce.get(command) ?? failAlways.get(command);
    if (injected) {
      failOnce.delete(command);
      // Failures arrive a moment later, as real ones do.
      await sleep(0);
      fail(
        injected.code,
        injected.message ?? "Simulated by the browser journey fake.",
        injected.details,
      );
    }
    const handler = commands[command];
    if (!handler)
      fail("internal", `This build has no command named ${command}.`, {
        command,
      });
    return wireCopy(await handler(wireArgs));
  }

  const control: FakeControl = {
    sentinel: SENTINEL,
    failNext: (command, failure) => {
      failOnce.set(command, wireCopy(failure));
    },
    failAlways: (command, failure) => {
      failAlways.set(command, wireCopy(failure));
    },
    clearFailures: () => {
      failOnce.clear();
      failAlways.clear();
    },
    setScanStepMs: (ms) => {
      scanStepMs = ms;
    },
    setSkipped: (entries) => {
      skipped = wireCopy(entries);
    },
    setWriter: (behaviour) => {
      writer = wireCopy(behaviour);
    },
    externalEdit: (relativePath, content) => {
      const entry = files.get(relativePath);
      if (!entry) return;
      files.set(relativePath, { ...entry, content, modifiedAtMs: Date.now() });
    },
    externalDelete: (relativePath) => {
      files.delete(relativePath);
    },
    expirePlans: () => {
      for (const stored of plans.values())
        stored.plan.expiresAt = Date.now() - 1;
    },
    readFile: (relativePath) => files.get(relativePath)?.content ?? null,
    listFiles: () => Array.from(files.keys()).sort(),
    calls: () => invoked.slice(),
    lastScan: () => wireCopy(lastScan),
  };

  scope.isTauri = true;
  scope.__TAURI_INTERNALS__ = {
    invoke,
    transformCallback(callback, _once) {
      const id = nextCallbackId++;
      callbacks.set(id, callback);
      return id;
    },
    unregisterCallback(id) {
      callbacks.delete(id);
    },
  };
  scope.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener(event, eventId) {
      listeners.get(event)?.delete(eventId);
    },
  };
  scope.__folioFake = control;

  if (options.preIndexed) {
    authorized = true;
    ready = (async () => {
      for (const path of Array.from(files.keys()).sort())
        await indexDocument(path);
    })();
  }
}

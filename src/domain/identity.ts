import {
  HASH_ALGORITHM,
  type DocumentId,
  type EmbeddingSpace,
  type EmbeddingSpaceFingerprint,
  type MediaType,
  type RelativePath,
  type WorkspaceId,
} from "./contracts";
import { folioError } from "./errors";
import { hashText } from "./hash";

const CONTROL = /[\u0000-\u001f\u007f]/;
/** Characters Windows rejects in a path segment. Checked for new destinations. */
const NOT_PORTABLE = /[<>:"|?*\\]/;

/**
 * Validate and canonicalize a workspace-relative path.
 *
 * NFC normalization matters: macOS hands back decomposed filenames, so the same
 * Filipino filename would otherwise produce two identities on two platforms.
 * Anything ambiguous is rejected rather than repaired, because a repaired path
 * would silently identify a different file.
 */
export function normalizeRelativePath(raw: string): RelativePath {
  if (typeof raw !== "string" || raw.length === 0) {
    throw folioError(
      "pathNotRelative",
      "A relative document path is required.",
    );
  }
  const value = raw.normalize("NFC");
  if (CONTROL.test(value)) {
    throw folioError(
      "pathNotRelative",
      "A document path cannot contain control characters.",
    );
  }
  if (value.includes("\\")) {
    throw folioError("pathNotRelative", "Use '/' to separate path segments.", {
      path: raw,
    });
  }
  if (value.startsWith("/") || /^[A-Za-z]:/.test(value)) {
    throw folioError(
      "pathNotRelative",
      "An absolute path cannot identify a document inside an authorized folder.",
      { path: raw },
    );
  }
  const segments = value.split("/");
  for (const segment of segments) {
    if (segment.length === 0) {
      throw folioError(
        "pathNotRelative",
        "A document path cannot contain an empty segment.",
        { path: raw },
      );
    }
    if (segment === "." || segment === "..") {
      throw folioError(
        "pathEscapesWorkspace",
        "A document path cannot navigate outside the authorized folder.",
        { path: raw },
      );
    }
  }
  return segments.join("/");
}

/**
 * Additional check for a path Folio would create: reject names Windows cannot
 * store, so a plan previewed on macOS does not fail halfway through on Windows.
 */
export function assertPortableDestination(raw: string): RelativePath {
  const path = normalizeRelativePath(raw);
  for (const segment of path.split("/")) {
    if (NOT_PORTABLE.test(segment) || /[ .]$/.test(segment)) {
      throw folioError(
        "operationUnsupported",
        `'${segment}' cannot be stored on every supported platform. Choose another name.`,
        { path },
      );
    }
  }
  return path;
}

export function assertWorkspaceId(value: string): WorkspaceId {
  if (!value || value.includes(":") || CONTROL.test(value)) {
    throw folioError(
      "workspaceNotAuthorized",
      "A workspace identity is required.",
    );
  }
  return value;
}

/**
 * Workspace identity derived from the canonical root path, so the same folder
 * keeps its identity across restarts. The native core is authoritative; this
 * implementation exists so both languages can be checked against one fixture.
 */
export async function workspaceIdForPath(
  canonicalRootPath: string,
): Promise<WorkspaceId> {
  const hash = await hashText(canonicalRootPath.normalize("NFC"));
  return hash.slice(HASH_ALGORITHM.length + 1);
}

/** `${workspaceId}:${relativePath}` — reversible and never lossy. */
export function documentIdFor(
  workspaceId: WorkspaceId,
  relativePath: string,
): DocumentId {
  return `${assertWorkspaceId(workspaceId)}:${normalizeRelativePath(relativePath)}`;
}

export function parseDocumentId(id: DocumentId): {
  workspaceId: WorkspaceId;
  relativePath: RelativePath;
} {
  const separator = id.indexOf(":");
  if (separator <= 0) {
    throw folioError("pathNotRelative", "Malformed document identity.", { id });
  }
  return {
    workspaceId: assertWorkspaceId(id.slice(0, separator)),
    relativePath: normalizeRelativePath(id.slice(separator + 1)),
  };
}

export function mediaTypeForPath(relativePath: string): MediaType | undefined {
  const name = relativePath.split("/").at(-1) ?? "";
  const extension = name.includes(".")
    ? name.slice(name.lastIndexOf(".") + 1).toLowerCase()
    : "";
  if (extension === "md" || extension === "markdown") return "text/markdown";
  if (extension === "txt") return "text/plain";
  if (extension === "pdf") return "application/pdf";
  return undefined;
}

function escapeField(value: string): string {
  return value.replace(/%/g, "%25").replace(/\//g, "%2F");
}

/**
 * Canonical single-string identity of a vector space. Two indexes may only be
 * compared when their fingerprints are equal.
 */
export function embeddingSpaceFingerprint(
  space: EmbeddingSpace,
): EmbeddingSpaceFingerprint {
  if (!Number.isInteger(space.dimensions) || space.dimensions <= 0) {
    throw folioError(
      "embeddingSpaceMismatch",
      "An embedding space needs a positive dimension count.",
    );
  }
  return [
    "folio-space-v1",
    escapeField(space.modelId),
    escapeField(space.revision),
    escapeField(space.quantization),
    String(space.dimensions),
    escapeField(space.preprocessingFingerprint),
  ].join("/");
}

export function sameEmbeddingSpace(
  a: EmbeddingSpace,
  b: EmbeddingSpace,
): boolean {
  return embeddingSpaceFingerprint(a) === embeddingSpaceFingerprint(b);
}

/** Refuse to compare vectors produced by different embedding spaces. */
export function assertSameEmbeddingSpace(
  a: EmbeddingSpace,
  b: EmbeddingSpace,
): void {
  const left = embeddingSpaceFingerprint(a);
  const right = embeddingSpaceFingerprint(b);
  if (left !== right) {
    throw folioError(
      "embeddingSpaceMismatch",
      "These vectors come from different embedding spaces. Rebuild the index before comparing them.",
      { expected: left, observed: right },
    );
  }
}

import {
  FOLIO_ERROR_CODES,
  type FolioErrorCode,
  type FolioErrorDetails,
  type FolioErrorPayload,
} from "./contracts";

const KNOWN_CODES: ReadonlySet<string> = new Set(FOLIO_ERROR_CODES);

/** True for a code this build of the boundary knows how to act on. */
export function isFolioErrorCode(value: unknown): value is FolioErrorCode {
  return typeof value === "string" && KNOWN_CODES.has(value);
}

/**
 * The single failure type on the boundary. Callers branch on `code`; `message`
 * is prose for the user. Native commands reject with the same payload shape.
 */
export class FolioError extends Error {
  readonly code: FolioErrorCode;
  readonly details?: FolioErrorDetails;

  constructor(
    code: FolioErrorCode,
    message: string,
    details?: FolioErrorDetails,
  ) {
    super(message);
    this.name = "FolioError";
    this.code = code;
    if (details) this.details = details;
  }

  toPayload(): FolioErrorPayload {
    return this.details
      ? { code: this.code, message: this.message, details: this.details }
      : { code: this.code, message: this.message };
  }
}

export function folioError(
  code: FolioErrorCode,
  message: string,
  details?: FolioErrorDetails,
): FolioError {
  return new FolioError(code, message, details);
}

export function isFolioError(value: unknown): value is FolioError {
  return value instanceof FolioError;
}

/** True when `value` has the wire shape a native command rejects with. */
export function isFolioErrorPayload(
  value: unknown,
): value is FolioErrorPayload {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.code === "string" && typeof candidate.message === "string"
  );
}

/** Keep only string details, stringifying anything a caller got wrong. */
function toDetails(value: unknown): FolioErrorDetails | undefined {
  if (typeof value !== "object" || value === null) return undefined;
  const details: FolioErrorDetails = {};
  for (const [key, entry] of Object.entries(value as Record<string, unknown>)) {
    details[key] = typeof entry === "string" ? entry : String(entry);
  }
  return Object.keys(details).length ? details : undefined;
}

/**
 * Normalize anything thrown or rejected into a `FolioError`. A native payload
 * keeps its code; a code this build does not know becomes `internal` with the
 * reported code kept as context, so an unrecognized failure is never presented
 * as a specific, actionable one.
 */
export function toFolioError(cause: unknown): FolioError {
  if (isFolioError(cause)) return cause;
  if (isFolioErrorPayload(cause)) {
    const details = toDetails(cause.details);
    if (isFolioErrorCode(cause.code)) {
      return new FolioError(cause.code, cause.message, details);
    }
    return new FolioError("internal", cause.message, {
      ...details,
      reportedCode: String(cause.code),
    });
  }
  if (cause instanceof Error && cause.name === "AbortError") {
    return new FolioError("cancelled", "The request was cancelled.");
  }
  return new FolioError(
    "internal",
    cause instanceof Error ? cause.message : String(cause),
  );
}

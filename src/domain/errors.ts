import type {
  FolioErrorCode,
  FolioErrorDetails,
  FolioErrorPayload,
} from "./contracts";

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

/**
 * Normalize anything thrown or rejected into a `FolioError`. A native payload
 * keeps its code; anything else becomes `internal` rather than being presented
 * as a specific, actionable failure.
 */
export function toFolioError(cause: unknown): FolioError {
  if (isFolioError(cause)) return cause;
  if (isFolioErrorPayload(cause)) {
    return new FolioError(
      cause.code as FolioErrorCode,
      cause.message,
      cause.details,
    );
  }
  if (cause instanceof Error && cause.name === "AbortError") {
    return new FolioError("cancelled", "The request was cancelled.");
  }
  return new FolioError(
    "internal",
    cause instanceof Error ? cause.message : String(cause),
  );
}

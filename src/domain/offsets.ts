import {
  SOURCE_OFFSET_UNIT,
  type ContentHash,
  type DocumentId,
  type SourcePassage,
} from "./contracts";
import { folioError } from "./errors";

const encoder = new TextEncoder();
const strictDecoder = new TextDecoder("utf-8", { fatal: true });

/** UTF-8 byte length — the unit every source offset on the boundary uses. */
export function utf8Length(text: string): number {
  return encoder.encode(text).length;
}

function isHighSurrogate(code: number): boolean {
  return code >= 0xd800 && code <= 0xdbff;
}

function isLowSurrogate(code: number): boolean {
  return code >= 0xdc00 && code <= 0xdfff;
}

/**
 * Convert a JavaScript string index (UTF-16 code units) into the UTF-8 byte
 * offset used on the boundary. JavaScript indexes are never sent across it.
 */
export function utf16IndexToUtf8Offset(text: string, index: number): number {
  if (!Number.isInteger(index) || index < 0 || index > text.length) {
    throw folioError("internal", "A string index is outside the document.", {
      index,
      length: text.length,
    });
  }
  if (
    index > 0 &&
    index < text.length &&
    isHighSurrogate(text.charCodeAt(index - 1)) &&
    isLowSurrogate(text.charCodeAt(index))
  ) {
    throw folioError(
      "internal",
      "A string index cannot split a surrogate pair.",
      { index },
    );
  }
  return encoder.encode(text.slice(0, index)).length;
}

/** Inverse of `utf16IndexToUtf8Offset`; refuses offsets inside a character. */
export function utf8OffsetToUtf16Index(text: string, offset: number): number {
  const bytes = encoder.encode(text);
  if (!Number.isInteger(offset) || offset < 0 || offset > bytes.length) {
    throw folioError("internal", "A source offset is outside the document.", {
      offset,
      sizeBytes: bytes.length,
    });
  }
  try {
    return strictDecoder.decode(bytes.subarray(0, offset)).length;
  } catch {
    throw folioError(
      "internal",
      "A source offset must fall on a character boundary.",
      { offset },
    );
  }
}

/** The excerpt between two UTF-8 byte offsets, or a typed error. */
export function sliceByUtf8Offsets(
  text: string,
  start: number,
  end: number,
): string {
  const bytes = encoder.encode(text);
  if (
    !Number.isInteger(start) ||
    !Number.isInteger(end) ||
    start < 0 ||
    end < start ||
    end > bytes.length
  ) {
    throw folioError("internal", "A source range is outside the document.", {
      start,
      end,
      sizeBytes: bytes.length,
    });
  }
  try {
    return strictDecoder.decode(bytes.subarray(start, end));
  } catch {
    throw folioError(
      "internal",
      "A source range must fall on character boundaries.",
      { start, end },
    );
  }
}

/** Build a passage from a UTF-16 range, converting to the boundary unit. */
export function passageFromUtf16Range(input: {
  documentId: DocumentId;
  documentContentHash: ContentHash;
  text: string;
  startIndex: number;
  endIndex: number;
  page?: number;
}): SourcePassage {
  const start = utf16IndexToUtf8Offset(input.text, input.startIndex);
  const end = utf16IndexToUtf8Offset(input.text, input.endIndex);
  const passage: SourcePassage = {
    documentId: input.documentId,
    documentContentHash: input.documentContentHash,
    offsetUnit: SOURCE_OFFSET_UNIT,
    start,
    end,
    text: input.text.slice(input.startIndex, input.endIndex),
  };
  if (input.page !== undefined) passage.page = input.page;
  return passage;
}

/**
 * Check a passage against the document revision it claims to come from. Used
 * before showing evidence or sending it to a model.
 */
export function assertPassageMatches(
  passage: SourcePassage,
  text: string,
  contentHash: ContentHash,
): void {
  if (passage.offsetUnit !== SOURCE_OFFSET_UNIT) {
    throw folioError(
      "evidenceInvalid",
      `Source offsets must be ${SOURCE_OFFSET_UNIT} offsets.`,
      { offsetUnit: String(passage.offsetUnit) },
    );
  }
  if (passage.documentContentHash !== contentHash) {
    throw folioError(
      "evidenceInvalid",
      "This passage was located in an earlier revision of the document.",
      { documentId: passage.documentId },
    );
  }
  if (sliceByUtf8Offsets(text, passage.start, passage.end) !== passage.text) {
    throw folioError(
      "evidenceInvalid",
      "This passage does not match the document text it points at.",
      { documentId: passage.documentId, start: passage.start },
    );
  }
}

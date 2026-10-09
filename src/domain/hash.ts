import { HASH_ALGORITHM, type ContentHash } from "./contracts";

const HEX = /^[0-9a-f]{64}$/;

/** `sha256:<64 lowercase hex>` over the UTF-8 bytes of `text`. */
export async function hashText(text: string): Promise<ContentHash> {
  return hashBytes(new TextEncoder().encode(text));
}

/** `sha256:<64 lowercase hex>` over exact bytes. */
export async function hashBytes(bytes: Uint8Array): Promise<ContentHash> {
  const digest = await crypto.subtle.digest(
    "SHA-256",
    bytes.slice().buffer as ArrayBuffer,
  );
  const hex = Array.from(new Uint8Array(digest))
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
  return `${HASH_ALGORITHM}:${hex}`;
}

/** True for a well-formed `sha256:<64 lowercase hex>` value. */
export function isContentHash(value: unknown): value is ContentHash {
  if (typeof value !== "string") return false;
  const separator = value.indexOf(":");
  if (separator < 0) return false;
  return (
    value.slice(0, separator) === HASH_ALGORITHM &&
    HEX.test(value.slice(separator + 1))
  );
}

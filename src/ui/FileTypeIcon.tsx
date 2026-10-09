import type { DocumentRecord } from "../domain/contracts";

const KINDS = {
  "application/pdf": { className: "file-type-pdf", label: "PDF" },
  "text/markdown": { className: "file-type-markdown", label: "MD" },
} as const;

/**
 * The brandkit mockup's file-type badge: a soft tile with the type's short
 * name. Decorative: the file name and the type text beside it carry the
 * meaning.
 */
export function FileTypeIcon({
  mediaType,
  size = 24,
}: {
  mediaType: DocumentRecord["mediaType"];
  size?: 20 | 24;
}) {
  const { className, label } = KINDS[mediaType as keyof typeof KINDS] ?? {
    className: "file-type-text",
    label: "TXT",
  };
  return (
    <span
      className={`file-type-icon ${className}${size === 20 ? " file-type-small" : ""}`}
      aria-hidden="true"
    >
      {label}
    </span>
  );
}

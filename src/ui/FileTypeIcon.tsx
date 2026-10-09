import { AlignLeft, FileText, Hash } from "lucide-react";
import type { DocumentRecord } from "../domain/contracts";

const KINDS = {
  "application/pdf": { className: "file-type-pdf", Icon: FileText },
  "text/markdown": { className: "file-type-markdown", Icon: Hash },
} as const;

/**
 * Coloured file-type tile, as in the brandbook. Decorative: the file name and
 * the type text beside it carry the meaning.
 */
export function FileTypeIcon({
  mediaType,
  size = 24,
}: {
  mediaType: DocumentRecord["mediaType"];
  size?: 20 | 24;
}) {
  const { className, Icon } = KINDS[mediaType as keyof typeof KINDS] ?? {
    className: "file-type-text",
    Icon: AlignLeft,
  };
  return (
    <span
      className={`file-type-icon ${className}`}
      style={{ width: size, height: size }}
      aria-hidden="true"
    >
      <Icon size={Math.round(size * 0.6)} strokeWidth={2.25} />
    </span>
  );
}

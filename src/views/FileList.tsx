import { FileText } from "lucide-react";
import { useState, type FocusEvent, type KeyboardEvent } from "react";
import type { DocumentRecord } from "../domain/contracts";
import { ListRow } from "../ui/ListRow";
import { fileKind, folderOf, formatBytes } from "./format";

interface FileListProps {
  label: string;
  documents: DocumentRecord[];
  selectedId: string | undefined;
  onSelect: (document: DocumentRecord) => void;
}

/**
 * Arrow keys, Home and End move between rows; Enter or Space opens one. The
 * single Tab stop follows the focused row, so Tab and Shift+Tab come back to it.
 */
export function FileList({
  label,
  documents,
  selectedId,
  onSelect,
}: FileListProps) {
  const [focusedId, setFocusedId] = useState<string>();

  function onFocus(event: FocusEvent<HTMLDivElement>) {
    const id = (event.target as HTMLElement).dataset.documentId;
    if (id) setFocusedId(id);
  }

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const rows = [
      ...event.currentTarget.querySelectorAll<HTMLElement>('[role="option"]'),
    ];
    const index = rows.indexOf(document.activeElement as HTMLElement);
    const next =
      event.key === "ArrowDown"
        ? index + 1
        : event.key === "ArrowUp"
          ? index - 1
          : event.key === "Home"
            ? 0
            : event.key === "End"
              ? rows.length - 1
              : null;
    if (next === null || !rows.length) return;
    event.preventDefault();
    rows[Math.max(0, Math.min(rows.length - 1, next))].focus();
  }

  const has = (id: string | undefined) =>
    id !== undefined && documents.some((item) => item.id === id);
  const tabStop = has(focusedId)
    ? focusedId
    : has(selectedId)
      ? selectedId
      : documents[0]?.id;

  return (
    <div
      className="file-list"
      role="listbox"
      aria-label={label}
      onKeyDown={onKeyDown}
      onFocus={onFocus}
    >
      {documents.map((document) => (
        <ListRow
          key={document.id}
          icon={<FileText size={20} />}
          title={document.name}
          subtitle={folderOf(document.relativePath)}
          meta={
            <>
              <span>{fileKind(document)}</span>
              <span className="tabular">{formatBytes(document.sizeBytes)}</span>
            </>
          }
          selected={document.id === selectedId}
          tabbable={document.id === tabStop}
          dataId={document.id}
          onSelect={() => onSelect(document)}
        />
      ))}
    </div>
  );
}

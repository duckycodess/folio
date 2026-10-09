import { useState, type FocusEvent, type KeyboardEvent } from "react";
import type { DocumentRecord } from "../domain/contracts";
import { FileTypeIcon } from "../ui/FileTypeIcon";
import { ListRow } from "../ui/ListRow";
import { fileKind, folderOf, formatBytes, formatDate } from "./format";

interface FileListProps {
  label: string;
  documents: DocumentRecord[];
  selectedId: string | undefined;
  onSelect: (document: DocumentRecord) => void;
}

/**
 * The file table: name, location, type, modified and size. Columns drop out
 * as the table narrows (see `.file-table` in components.css), and the location
 * moves under the name.
 *
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
    <div className="file-table">
      {/* Visual column headings; each row's spoken name carries the same facts. */}
      <div className="file-table-head" aria-hidden="true">
        <span className="file-col-name">Name</span>
        <span className="file-col-location">Location</span>
        <span className="file-col-type">Type</span>
        <span className="file-col-modified">Modified</span>
        <span className="file-col-size">Size</span>
      </div>
      <div
        className="file-list"
        role="listbox"
        aria-label={label}
        onKeyDown={onKeyDown}
        onFocus={onFocus}
      >
        {documents.map((document) => {
          const location = folderOf(document.relativePath);
          const kind = fileKind(document);
          const size = formatBytes(document.sizeBytes);
          const modified =
            document.modifiedAtMs === undefined
              ? undefined
              : formatDate(document.modifiedAtMs);
          return (
            <ListRow
              key={document.id}
              icon={<FileTypeIcon mediaType={document.mediaType} />}
              title={document.name}
              subtitle={location}
              cells={
                <>
                  <span className="file-col-location">{location}</span>
                  <span className="file-col-type">{kind}</span>
                  <span className="file-col-modified tabular">
                    {modified ?? "—"}
                  </span>
                  <span className="file-col-size tabular">{size}</span>
                </>
              }
              label={[
                document.name,
                location,
                kind,
                modified ? `modified ${modified}` : undefined,
                size,
              ]
                .filter(Boolean)
                .join(", ")}
              tooltip={document.relativePath}
              selected={document.id === selectedId}
              tabbable={document.id === tabStop}
              dataId={document.id}
              onSelect={() => onSelect(document)}
            />
          );
        })}
      </div>
    </div>
  );
}

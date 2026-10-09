import {
  useId,
  useState,
  type FocusEvent,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import type { DocumentRecord, SearchResult } from "../domain/contracts";
import { FileTypeIcon } from "../ui/FileTypeIcon";
import { ListRow } from "../ui/ListRow";
import { RowMenu, type RowMenuItem } from "../ui/RowMenu";
import { fileKind, folderOf, formatBytes, formatDate } from "./format";

interface FileListProps {
  label: string;
  documents: DocumentRecord[];
  selectedId: string | undefined;
  onSelect: (document: DocumentRecord) => void;
  /** Actions in each row's ⋯ menu. Without it, rows have no menu. */
  actions?: (document: DocumentRecord) => RowMenuItem[];
  /**
   * Extra lines under a row's main line, such as search evidence. They are
   * also the row button's accessible description.
   */
  renderDetail?: (
    document: DocumentRecord,
    result: SearchResult | undefined,
  ) => ReactNode;
  /** The search results behind `documents`, passed to `renderDetail`. */
  results?: SearchResult[];
}

/**
 * The file table: name, location, type, modified and size. Columns drop out
 * as the table narrows (see `.file-table` in components.css), and the location
 * moves under the name.
 *
 * Arrow keys, Home and End move between rows; Enter or Space opens one. The
 * single Tab stop follows the focused row, so Tab and Shift+Tab come back to
 * it; from there, Tab reaches that row's ⋯ menu.
 *
 * A row's spoken name is the text it shows, column by column, rather than a
 * sentence of its own: the columns change with the table's width, and a name
 * that does not contain the visible text is one a voice-control user cannot
 * say. The heading row is hidden from assistive tech, so the date says
 * "modified" in hidden words.
 */
export function FileList({
  label,
  documents,
  selectedId,
  onSelect,
  actions,
  renderDetail,
  results,
}: FileListProps) {
  const [focusedId, setFocusedId] = useState<string>();
  const detailPrefix = useId();
  const byId = new Map(results?.map((result) => [result.document.id, result]));

  function onFocus(event: FocusEvent<HTMLUListElement>) {
    const row = (event.target as HTMLElement).closest<HTMLElement>(
      "[data-row-id]",
    );
    if (row?.dataset.rowId) setFocusedId(row.dataset.rowId);
  }

  function onKeyDown(event: KeyboardEvent<HTMLUListElement>) {
    if (!(event.target as HTMLElement).classList.contains("list-row")) return;
    const rows = [
      ...event.currentTarget.querySelectorAll<HTMLElement>(".list-row"),
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
    <div className={`file-table${actions ? " has-actions" : ""}`}>
      {/* Visual column headings; each row's spoken name carries the same facts. */}
      <div className="file-table-head" aria-hidden="true">
        <span className="file-col-name">Name</span>
        <span className="file-col-location">Location</span>
        <span className="file-col-type">Type</span>
        <span className="file-col-modified">Modified</span>
        <span className="file-col-size">Size</span>
      </div>
      <ul
        className="file-list"
        aria-label={label}
        onKeyDown={onKeyDown}
        onFocus={onFocus}
      >
        {documents.map((document, index) => {
          const location = folderOf(document.relativePath);
          const kind = fileKind(document);
          const size = formatBytes(document.sizeBytes);
          const modified =
            document.modifiedAtMs === undefined
              ? undefined
              : formatDate(document.modifiedAtMs);
          const detail = renderDetail?.(document, byId.get(document.id));
          const detailId = detail ? `${detailPrefix}-${index}` : undefined;
          const tabbable = document.id === tabStop;
          return (
            <li
              key={document.id}
              className="file-row"
              data-row-id={document.id}
            >
              <div className="file-row-line">
                <ListRow
                  icon={<FileTypeIcon mediaType={document.mediaType} />}
                  title={document.name}
                  subtitle={location}
                  cells={
                    <>
                      <span className="file-col-location">{location}</span>
                      <span className="file-col-type">{kind}</span>
                      {/* The heading row is hidden from assistive tech, so a
                          date carries its column in words that are spoken,
                          not shown. */}
                      <span className="file-col-modified tabular">
                        {modified ? (
                          <>
                            <span className="visually-hidden">modified </span>
                            {modified}
                          </>
                        ) : (
                          <>
                            <span aria-hidden="true">—</span>
                            <span className="visually-hidden">
                              no modified date
                            </span>
                          </>
                        )}
                      </span>
                      <span className="file-col-size tabular">{size}</span>
                    </>
                  }
                  tooltip={document.relativePath}
                  selected={document.id === selectedId}
                  describedBy={detailId}
                  tabbable={tabbable}
                  dataId={document.id}
                  onSelect={() => onSelect(document)}
                />
                {actions && (
                  <RowMenu
                    label={`Actions for ${document.name}`}
                    items={actions(document)}
                    tabbable={tabbable}
                  />
                )}
              </div>
              {detail && (
                <div id={detailId} className="file-row-detail">
                  {detail}
                </div>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}

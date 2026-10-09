import {
  useId,
  useLayoutEffect,
  useState,
  type CSSProperties,
  type FocusEvent,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import {
  fileColumnsTemplate,
  NAME_MIN_WIDTH,
  nameColumnWidth,
  visibleColumns,
} from "../app/fileColumns";
import { useElementWidth } from "../app/useElementWidth";
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
 * one at a time, in priority order (size, then modified, then type, then
 * location — see `../app/fileColumns.ts`), as the table's own width shrinks,
 * so the name is never the column that gets crushed (#67). Once location
 * drops, it moves under the name instead.
 *
 * Arrow keys, Home and End move between rows; Enter or Space opens one. The
 * single Tab stop follows the focused row, so Tab and Shift+Tab come back to
 * it; from there, Tab reaches that row's ⋯ menu.
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
  // Falls back to showing every column until the first measurement lands.
  const [tableRef, tableWidth] = useElementWidth<HTMLDivElement>(1200);
  // What a row's grid can't use (its padding and the ⋯ menu), and the room
  // the longest name needs. Neither depends on which columns show, so
  // measuring them after each render settles at once.
  const [fit, setFit] = useState({ chrome: 32, name: NAME_MIN_WIDTH });
  useLayoutEffect(() => {
    const table = tableRef.current;
    const row = table?.querySelector<HTMLElement>(".list-row");
    if (!table || !row) return;
    const style = getComputedStyle(row);
    const grid =
      row.clientWidth -
      parseFloat(style.paddingLeft) -
      parseFloat(style.paddingRight);
    const chrome = Math.round(table.clientWidth - grid);
    let longest = 0;
    for (const title of table.querySelectorAll<HTMLElement>(
      ".list-row-title",
    )) {
      const main = title.closest(".list-row-main");
      const indent = main
        ? title.getBoundingClientRect().left - main.getBoundingClientRect().left
        : 0;
      longest = Math.max(longest, indent + title.scrollWidth);
    }
    const name = nameColumnWidth(Math.ceil(longest));
    setFit((previous) =>
      previous.chrome === chrome && previous.name === name
        ? previous
        : { chrome, name },
    );
  });
  const shown = visibleColumns(tableWidth - fit.chrome, fit.name);
  const showLocationColumn = shown.includes("location");
  const showType = shown.includes("type");
  const showModified = shown.includes("modified");
  const showSize = shown.includes("size");
  const columnsStyle = {
    "--file-columns": fileColumnsTemplate(shown),
  } as CSSProperties;

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
    <div
      ref={tableRef}
      className={`file-table${actions ? " has-actions" : ""}${showLocationColumn ? "" : " file-table-compact"}`}
      style={columnsStyle}
    >
      {/* Visual column headings; each row's spoken name carries the same facts. */}
      <div className="file-table-head" aria-hidden="true">
        <span className="file-col-name">Name</span>
        {showLocationColumn && (
          <span className="file-col-location">Location</span>
        )}
        {showType && <span className="file-col-type">Type</span>}
        {showModified && <span className="file-col-modified">Modified</span>}
        {showSize && <span className="file-col-size">Size</span>}
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
                      {showLocationColumn && (
                        <span className="file-col-location">{location}</span>
                      )}
                      {showType && (
                        <span className="file-col-type">{kind}</span>
                      )}
                      {showModified && (
                        <span className="file-col-modified tabular">
                          {modified ?? "—"}
                        </span>
                      )}
                      {showSize && (
                        <span className="file-col-size tabular">{size}</span>
                      )}
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

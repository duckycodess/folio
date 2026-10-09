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
import { Star } from "lucide-react";
import { useElementWidth } from "../app/useElementWidth";
import type { DocumentRecord, SearchResult } from "../domain/contracts";
import { FileTypeIcon } from "../ui/FileTypeIcon";
import { ListRow } from "../ui/ListRow";
import { RowMenu, type RowMenuItem } from "../ui/RowMenu";
import { folderOf, formatDate } from "./format";

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
  /** Starred files; with `onToggleStar`, each row gets a star button. */
  isStarred?: (document: DocumentRecord) => boolean;
  onToggleStar?: (document: DocumentRecord) => void;
}

/**
 * The file table, as in the brandkit mockup: name, folder, modified and a
 * star. Columns drop out one at a time (modified, then folder — see
 * `../app/fileColumns.ts`) as the table's own width shrinks, so the name is
 * never the column that gets crushed (#67). Once the folder drops, it moves
 * under the name instead.
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
  isStarred,
  onToggleStar,
}: FileListProps) {
  const [focusedId, setFocusedId] = useState<string>();
  const detailPrefix = useId();
  const byId = new Map(results?.map((result) => [result.document.id, result]));
  // Falls back to showing every column until the first measurement lands.
  const [tableRef, tableWidth, table] = useElementWidth<HTMLDivElement>(1200);
  // What a row's grid can't use (its padding and the ⋯ menu), and the room
  // the longest name needs. Neither depends on which columns show, so
  // measuring them after each render settles at once.
  const [fit, setFit] = useState({ chrome: 32, name: NAME_MIN_WIDTH });
  useLayoutEffect(() => {
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
  const showModified = shown.includes("modified");
  const stars = Boolean(isStarred && onToggleStar);
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
      className={`file-table${actions ? " has-actions" : ""}${stars ? " has-stars" : ""}${showLocationColumn ? "" : " file-table-compact"}`}
      style={columnsStyle}
    >
      {/* Visual column headings; each row's spoken name carries the same facts. */}
      <div className="file-table-head" aria-hidden="true">
        <span className="file-col-name">Name</span>
        {showLocationColumn && (
          <span className="file-col-location">Folder</span>
        )}
        {showModified && <span className="file-col-modified">Modified</span>}
      </div>
      <ul
        className="file-list"
        aria-label={label}
        onKeyDown={onKeyDown}
        onFocus={onFocus}
      >
        {documents.map((document, index) => {
          const location = folderOf(document.relativePath);
          const starred = isStarred?.(document) ?? false;
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
                        <span className="file-col-location">
                          <span className="pill">{location}</span>
                        </span>
                      )}
                      {/* The heading row is hidden from assistive tech, so a
                          date carries its column in words that are spoken,
                          not shown. */}
                      {showModified && (
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
                      )}
                    </>
                  }
                  tooltip={document.relativePath}
                  selected={document.id === selectedId}
                  describedBy={detailId}
                  tabbable={tabbable}
                  dataId={document.id}
                  onSelect={() => onSelect(document)}
                />
                {stars && (
                  <button
                    type="button"
                    className={`star-button${starred ? " is-starred" : ""}`}
                    aria-pressed={starred}
                    aria-label={`Star ${document.name}`}
                    title={starred ? "Starred" : "Star"}
                    tabIndex={tabbable ? 0 : -1}
                    onClick={() => onToggleStar?.(document)}
                  >
                    <Star
                      size={16}
                      aria-hidden="true"
                      fill={starred ? "currentColor" : "none"}
                    />
                  </button>
                )}
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

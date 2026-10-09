import { Pin, PinOff, X } from "lucide-react";
import { useId } from "react";
import type { HomeState } from "../app/useHome";
import type { DocumentRecord } from "../domain/contracts";
import {
  hasFilters,
  NO_FILTERS,
  type HomeFilters,
  type ModifiedFilter,
  type TypeFilter,
} from "../domain/homeFilters";
import { FileTypeIcon } from "../ui/FileTypeIcon";

const TYPES: [TypeFilter, string][] = [
  ["any", "Any type"],
  ["text/plain", "Text"],
  ["text/markdown", "Markdown"],
  ["application/pdf", "PDF"],
];

const MODIFIED: [ModifiedFilter, string][] = [
  ["any", "Any time"],
  ["today", "Today"],
  ["week", "Past week"],
  ["month", "Past month"],
];

/** "Top level" for the open folder's own files, otherwise the path. */
export function folderLabel(folder: string): string {
  return folder === "" ? "Top level" : folder.split("/").join(" / ");
}

/** Shown for a pinned or chosen folder that no longer holds any listed file. */
export function emptyFolderLabel(folder: string): string {
  return `${folderLabel(folder)} (no files)`;
}

/** Folder, File type and Modified filters, plus pinning the chosen folder. */
export function HomeFilterBar({
  home,
  folders,
}: {
  home: HomeState;
  folders: string[];
}) {
  const id = useId();
  const { filters } = home;
  const set = (change: Partial<HomeFilters>) =>
    home.setFilters({ ...filters, ...change });
  const pinned = filters.folder !== null && home.pins.includes(filters.folder);
  // A folder emptied by Organize or Move still has to show as chosen, or the
  // select would fall back to "All folders" while the filter hides everything.
  const gone =
    filters.folder && !folders.includes(filters.folder) ? filters.folder : null;
  return (
    <div className="home-filters" role="group" aria-label="Filters">
      <div className="filter">
        <label className="filter-label" htmlFor={`${id}-folder`}>
          Folder
        </label>
        <select
          id={`${id}-folder`}
          className="select"
          value={filters.folder ?? "*"}
          onChange={(event) =>
            set({
              folder: event.target.value === "*" ? null : event.target.value,
            })
          }
        >
          <option value="*">All folders</option>
          <option value="">Top level</option>
          {folders.map((folder) => (
            <option key={folder} value={folder}>
              {folderLabel(folder)}
            </option>
          ))}
          {gone !== null && (
            <option value={gone}>{emptyFolderLabel(gone)}</option>
          )}
        </select>
      </div>
      <div className="filter">
        <label className="filter-label" htmlFor={`${id}-type`}>
          File type
        </label>
        <select
          id={`${id}-type`}
          className="select"
          value={filters.type}
          onChange={(event) => set({ type: event.target.value as TypeFilter })}
        >
          {TYPES.map(([value, label]) => (
            <option key={value} value={value}>
              {label}
            </option>
          ))}
        </select>
      </div>
      <div className="filter">
        <label className="filter-label" htmlFor={`${id}-modified`}>
          Modified
        </label>
        <select
          id={`${id}-modified`}
          className="select"
          value={filters.modified}
          onChange={(event) =>
            set({ modified: event.target.value as ModifiedFilter })
          }
        >
          {MODIFIED.map(([value, label]) => (
            <option key={value} value={value}>
              {label}
            </option>
          ))}
        </select>
      </div>
      {hasFilters(filters) && (
        <div className="filter-actions">
          {filters.folder !== null && (
            <button
              type="button"
              className="button button-ghost"
              aria-pressed={pinned}
              onClick={() => home.togglePin(filters.folder!)}
            >
              {pinned ? (
                <PinOff size={16} aria-hidden="true" />
              ) : (
                <Pin size={16} aria-hidden="true" />
              )}
              {pinned ? "Unpin folder" : "Pin folder"}
            </button>
          )}
          {hasFilters(filters) && (
            <button
              type="button"
              className="link-button"
              onClick={() => home.setFilters(NO_FILTERS)}
            >
              Clear filters
            </button>
          )}
        </div>
      )}
    </div>
  );
}

/** Pinned folders as quick filters. Shown only once something is pinned. */
export function PinnedFolders({
  home,
  folders,
}: {
  home: HomeState;
  folders: string[];
}) {
  if (!home.pins.length) return null;
  return (
    <section className="home-strip" aria-labelledby="pinned-heading">
      <h2 id="pinned-heading" className="strip-title">
        Pinned folders
      </h2>
      <ul className="chip-list">
        {home.pins.map((folder) => {
          const active = home.filters.folder === folder;
          return (
            <li key={folder} className="chip-item">
              <button
                type="button"
                className={`chip${active ? " is-active" : ""}`}
                aria-pressed={active}
                onClick={() =>
                  home.setFilters({
                    ...home.filters,
                    folder: active ? null : folder,
                  })
                }
              >
                <Pin size={14} aria-hidden="true" />
                {folder === "" || folders.includes(folder)
                  ? folderLabel(folder)
                  : emptyFolderLabel(folder)}
              </button>
              <button
                type="button"
                className="chip-remove"
                aria-label={`Unpin ${folderLabel(folder)}`}
                title="Unpin"
                onClick={() => home.togglePin(folder)}
              >
                <X size={14} aria-hidden="true" />
              </button>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

/** Files opened in Folio on this device, newest first. */
export function RecentFiles({
  documents,
  onOpen,
}: {
  documents: DocumentRecord[];
  onOpen: (document: DocumentRecord) => void;
}) {
  if (!documents.length) return null;
  return (
    <section className="home-strip" aria-labelledby="recent-heading">
      <h2 id="recent-heading" className="strip-title">
        Recent files
        <span className="strip-note">Opened in Folio on this device</span>
      </h2>
      <ul className="chip-list">
        {documents.map((document) => (
          <li key={document.id}>
            <button
              type="button"
              className="chip"
              title={document.relativePath}
              onClick={() => onOpen(document)}
            >
              <FileTypeIcon mediaType={document.mediaType} size={20} />
              <span className="chip-text">{document.name}</span>
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

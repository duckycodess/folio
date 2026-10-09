import { Pin, PinOff, X } from "lucide-react";
import { useId } from "react";
import type { HomeState } from "../app/useHome";
import type { DocumentRecord } from "../domain/contracts";
import {
  NO_FILTERS,
  passesFilters,
  type HomeFilters,
  type ModifiedFilter,
  type TypeFilter,
} from "../domain/homeFilters";
import { FileTypeIcon } from "../ui/FileTypeIcon";

const TYPES: [TypeFilter, string][] = [
  ["any", "File type"],
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

/**
 * Folder, File type and Modified filters, plus pinning the chosen folder and
 * Reset, as the brandkit mockup's compact row under search. Labels are
 * spoken, not shown: each select's first option names it.
 */
export function HomeFilterBar({
  home,
  folders,
  onReset,
}: {
  home: HomeState;
  folders: string[];
  /** Clears the filters and the search. */
  onReset: () => void;
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
        <label className="visually-hidden" htmlFor={`${id}-folder`}>
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
        <label className="visually-hidden" htmlFor={`${id}-type`}>
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
        <label className="visually-hidden" htmlFor={`${id}-modified`}>
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
        <button type="button" className="filter-reset" onClick={onReset}>
          Reset
        </button>
      </div>
    </div>
  );
}

/**
 * The brandkit mockup's folder cards: pinned folders, or, until something is
 * pinned, the three folders with the most files. Each one filters the list
 * to that folder.
 */
export function PinnedFolders({
  home,
  folders,
  documents,
}: {
  home: HomeState;
  folders: string[];
  documents: DocumentRecord[];
}) {
  // The same files the folder filter shows: subfolders included.
  const now = Date.now();
  const count = (folder: string) =>
    documents.filter((document) =>
      passesFilters(document, { ...NO_FILTERS, folder }, now),
    ).length;
  const pinned = home.pins.length > 0;
  const shown = pinned
    ? home.pins
    : [...folders].sort((a, b) => count(b) - count(a)).slice(0, 3);
  if (!shown.length) return null;
  return (
    <section className="home-folders" aria-labelledby="pinned-heading">
      <div className="section-head">
        <h2 id="pinned-heading" className="section-head-title">
          {pinned ? "Pinned folders" : "Folders"}
        </h2>
        <span className="section-head-note">
          {pinned ? "Your everyday spaces" : "Pin one to keep it here"}
        </span>
      </div>
      <ul className="folder-cards">
        {shown.map((folder) => {
          const active = home.filters.folder === folder;
          const files = count(folder);
          return (
            <li key={folder} className="folder-card-item">
              <button
                type="button"
                className={`folder-card${active ? " is-active" : ""}`}
                aria-pressed={active}
                onClick={() =>
                  home.setFilters({
                    ...home.filters,
                    folder: active ? null : folder,
                  })
                }
              >
                <span className="folder-icon" aria-hidden="true" />
                <span className="folder-card-name">
                  {folder === "" || folders.includes(folder)
                    ? folderLabel(folder)
                    : emptyFolderLabel(folder)}
                </span>
                <span className="folder-card-meta">
                  {files === 1 ? "1 file" : `${files} files`} ·{" "}
                  {active ? "Showing" : "Open folder"}{" "}
                  <span aria-hidden="true">↗</span>
                </span>
              </button>
              {pinned && (
                <button
                  type="button"
                  className="folder-card-unpin"
                  aria-label={`Unpin ${folderLabel(folder)}`}
                  title="Unpin"
                  onClick={() => home.togglePin(folder)}
                >
                  <X size={14} aria-hidden="true" />
                </button>
              )}
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

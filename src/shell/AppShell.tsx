import {
  FlaskConical,
  Folder,
  Folders,
  House,
  Sparkles,
  Waypoints,
  type LucideIcon,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useWorkspace } from "../app/useWorkspace";
import { Notice } from "../ui/Notice";
import { SearchField } from "../ui/SearchField";
import { AssistantView } from "../views/AssistantView";
import { DocumentPanel } from "../views/DocumentPanel";
import { FilesView } from "../views/FilesView";
import { GraphView } from "../views/GraphView";
import { HomeView } from "../views/HomeView";
import { ModelLabView } from "../views/ModelLabView";
import { OrganizeView } from "../views/OrganizeView";
import {
  currentPlatform,
  isSearchShortcut,
  PRIMARY_NAV,
  searchShortcutLabel,
  SECONDARY_NAV,
  type NavItem,
  type ViewId,
} from "./navigation";

const ICONS: Record<ViewId, LucideIcon> = {
  home: House,
  files: Folder,
  organize: Folders,
  graph: Waypoints,
  assistant: Sparkles,
  modelLab: FlaskConical,
};

const TITLES: Record<ViewId, string> = {
  home: "Overview",
  files: "Files",
  organize: "Organize",
  graph: "Graph",
  assistant: "Ask & Act",
  modelLab: "Model Lab",
};

/** Views whose content is a document list, so the reader panel sits beside it. */
const DOCUMENT_VIEWS = new Set<ViewId>(["home", "files", "organize", "graph"]);

export function AppShell() {
  const workspace = useWorkspace();
  const [view, setView] = useState<ViewId>("home");
  const searchInput = useRef<HTMLInputElement>(null);
  const platform = useMemo(currentPlatform, []);
  const showsDocument = DOCUMENT_VIEWS.has(view) && workspace.selected;

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      // An open modal handles its own Escape.
      if (
        event.key === "Escape" &&
        showsDocument &&
        !event.defaultPrevented &&
        !document.querySelector("dialog[open]")
      ) {
        closeDocument();
        return;
      }
      if (!isSearchShortcut(event, platform)) return;
      event.preventDefault();
      searchInput.current?.focus();
      searchInput.current?.select();
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  });

  // Closing the reader returns focus to the list row that opened it.
  function closeDocument() {
    const id = workspace.selected?.id;
    if (!id) return;
    workspace.clearSelection();
    requestAnimationFrame(() =>
      document
        .querySelector<HTMLElement>(`[data-document-id="${CSS.escape(id)}"]`)
        ?.focus(),
    );
  }

  function onSearch(query: string) {
    workspace.setQuery(query);
    if (view !== "home" && view !== "files") setView("home");
  }

  return (
    <div className={`app${showsDocument ? " has-document" : ""}`}>
      <a className="skip-link" href="#main">
        Skip to content
      </a>
      <aside className="sidebar">
        {/* Interim text wordmark until the logo SVG is exported. */}
        <span className="wordmark" role="img" aria-label="Folio">
          <span className="wordmark-full" aria-hidden="true">
            folio
          </span>
          <span className="wordmark-short" aria-hidden="true">
            f
          </span>
        </span>
        <nav aria-label="Main" className="nav">
          <NavList items={PRIMARY_NAV} current={view} onSelect={setView} />
        </nav>
        <div className="sidebar-footer">
          <nav aria-label="Settings" className="nav">
            <NavList items={SECONDARY_NAV} current={view} onSelect={setView} />
          </nav>
          <button
            type="button"
            className="status-pill"
            onClick={() => setView("modelLab")}
            aria-label="Local AI not set up. Open Model Lab."
            title="Local AI isn't set up. Open Model Lab."
          >
            <span className="status-dot" aria-hidden="true" />
            <span className="status-text">Local AI not set up</span>
          </button>
        </div>
      </aside>

      <div className="main-column">
        <header className="topbar">
          <nav aria-label="Breadcrumb" className="breadcrumb">
            <span>{workspace.workspace ? "Workspace" : "Sample files"}</span>
            <span aria-hidden="true">/</span>
            <span aria-current="page">{TITLES[view]}</span>
          </nav>
          <SearchField
            ref={searchInput}
            label="Search files"
            value={workspace.query}
            onChange={onSearch}
            placeholder="Search files, ideas, or projects"
            shortcut={searchShortcutLabel(platform)}
          />
        </header>

        <main id="main" className="main" tabIndex={-1}>
          {workspace.error && <Notice tone="danger">{workspace.error}</Notice>}
          {workspace.notice && (
            <Notice
              tone="warning"
              action={
                <button
                  type="button"
                  className="link-button"
                  onClick={workspace.dismissNotice}
                >
                  Dismiss
                </button>
              }
            >
              {workspace.notice}
            </Notice>
          )}
          {view === "home" && (
            <HomeView workspace={workspace} onNavigate={setView} />
          )}
          {view === "files" && <FilesView workspace={workspace} />}
          {view === "organize" && <OrganizeView workspace={workspace} />}
          {view === "graph" && <GraphView workspace={workspace} />}
          {view === "assistant" && <AssistantView onNavigate={setView} />}
          {view === "modelLab" && <ModelLabView />}
        </main>
      </div>

      {showsDocument && workspace.selected && (
        <DocumentPanel
          key={workspace.selected.id}
          document={workspace.selected}
          workspace={workspace}
          onClose={closeDocument}
          onNavigate={setView}
        />
      )}
    </div>
  );
}

function NavList({
  items,
  current,
  onSelect,
}: {
  items: NavItem[];
  current: ViewId;
  onSelect: (view: ViewId) => void;
}) {
  return (
    <ul className="nav-list">
      {items.map((item) => {
        const Icon = ICONS[item.id];
        const active = item.id === current;
        return (
          <li key={item.id}>
            <button
              type="button"
              className={`nav-item${active ? " is-active" : ""}`}
              aria-current={active ? "page" : undefined}
              aria-label={item.label}
              title={item.label}
              onClick={() => onSelect(item.id)}
            >
              <Icon size={20} aria-hidden="true" />
              <span className="nav-label" aria-hidden="true">
                {item.label}
              </span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}

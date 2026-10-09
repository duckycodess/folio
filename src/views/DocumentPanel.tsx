import { ArrowLeft, Sparkles, X } from "lucide-react";
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import type { WorkspaceState } from "../app/useWorkspace";
import type { DocumentRecord } from "../domain/contracts";
import type { ViewId } from "../shell/navigation";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { EmptyState } from "../ui/EmptyState";
import { FileTypeIcon } from "../ui/FileTypeIcon";
import { Progress } from "../ui/Progress";
import {
  fileKind,
  folderOf,
  formatBytes,
  formatModified,
  languageLabel,
} from "./format";

const TABS = ["Summary", "Details", "Related"] as const;
type Tab = (typeof TABS)[number];

interface DocumentPanelProps {
  document: DocumentRecord;
  workspace: WorkspaceState;
  onClose: () => void;
  onNavigate: (view: ViewId) => void;
}

export function DocumentPanel({
  document,
  workspace,
  onClose,
  onNavigate,
}: DocumentPanelProps) {
  const [tab, setTab] = useState<Tab>("Details");
  const tabRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const id = useId();
  const heading = useRef<HTMLHeadingElement>(null);

  // The panel is keyed by document, so this runs each time one opens. In
  // narrow windows the panel replaces the list, so the row that opened it is
  // hidden; elsewhere focus stays on the list unless it was lost (a Related
  // link replaced the previous panel).
  useEffect(() => {
    const active = window.document.activeElement;
    if (
      window.matchMedia("(max-width: 860px)").matches ||
      !active ||
      active === window.document.body
    )
      heading.current?.focus();
  }, []);

  function onTabKeyDown(event: KeyboardEvent, index: number) {
    const next =
      event.key === "ArrowRight"
        ? (index + 1) % TABS.length
        : event.key === "ArrowLeft"
          ? (index - 1 + TABS.length) % TABS.length
          : null;
    if (next === null) return;
    event.preventDefault();
    setTab(TABS[next]);
    tabRefs.current[next]?.focus();
  }

  return (
    <aside className="document-panel" aria-label={`${document.name} details`}>
      <div className="document-panel-header">
        <button
          type="button"
          className="icon-button document-back"
          aria-label="Back to files"
          onClick={onClose}
        >
          <ArrowLeft size={18} aria-hidden="true" />
        </button>
        <FileTypeIcon mediaType={document.mediaType} />
        <div className="document-heading">
          <h2
            ref={heading}
            className="document-title"
            title={document.name}
            tabIndex={-1}
          >
            {document.name}
          </h2>
          <p className="document-meta tabular">
            {fileKind(document)} · {formatBytes(document.sizeBytes)}
            {document.modifiedAtMs !== undefined &&
              ` · ${formatModified(document.modifiedAtMs)}`}
          </p>
        </div>
        <button
          type="button"
          className="icon-button document-close"
          aria-label="Close details"
          onClick={onClose}
        >
          <X size={18} aria-hidden="true" />
        </button>
      </div>

      <div role="tablist" aria-label="Document" className="tabs">
        {TABS.map((name, index) => (
          <button
            key={name}
            ref={(element) => {
              tabRefs.current[index] = element;
            }}
            type="button"
            role="tab"
            id={`${id}-tab-${name}`}
            // Only the selected tab's panel is rendered.
            aria-controls={tab === name ? `${id}-panel-${name}` : undefined}
            aria-selected={tab === name}
            tabIndex={tab === name ? 0 : -1}
            className={`tab${tab === name ? " is-active" : ""}`}
            onClick={() => setTab(name)}
            onKeyDown={(event) => onTabKeyDown(event, index)}
          >
            {name}
          </button>
        ))}
      </div>

      <div
        role="tabpanel"
        id={`${id}-panel-${tab}`}
        aria-labelledby={`${id}-tab-${tab}`}
        className="tab-panel"
        tabIndex={0}
      >
        {tab === "Summary" && (
          <EmptyState
            icon={<Sparkles size={24} />}
            title="Summaries need a local AI model"
            action={
              <Button
                variant="secondary"
                onClick={() => onNavigate("modelLab")}
              >
                Open Model Lab
              </Button>
            }
          >
            You can still read the whole file in Details.
          </EmptyState>
        )}

        {tab === "Details" && (
          <>
            <dl className="details-list">
              <dt>Location</dt>
              <dd title={document.relativePath}>
                {folderOf(document.relativePath)}
              </dd>
              <dt>Type</dt>
              <dd>{fileKind(document)}</dd>
              <dt>Language</dt>
              <dd>{languageLabel(document.language)}</dd>
              <dt>Size</dt>
              <dd className="tabular">{formatBytes(document.sizeBytes)}</dd>
            </dl>
            <h3 className="subsection-title">Contents</h3>
            {workspace.busy && document.content === undefined ? (
              <Progress label="Reading file" />
            ) : document.content !== undefined ? (
              <pre className="source-text">{document.content}</pre>
            ) : (
              <p className="muted">
                {document.mediaType === "application/pdf"
                  ? "Reading PDF text isn't available yet."
                  : "This file hasn't been read yet."}
              </p>
            )}
          </>
        )}

        {tab === "Related" &&
          (workspace.neighbors.length ? (
            <ul className="related-list">
              {workspace.neighbors.map((related) => (
                <li key={related.id}>
                  <button
                    type="button"
                    className="related-item"
                    onClick={() => workspace.selectDocument(related)}
                  >
                    <FileTypeIcon mediaType={related.mediaType} size={20} />
                    <span className="related-text">
                      <span className="related-name">{related.name}</span>
                      <span className="related-path">
                        {folderOf(related.relativePath)}
                      </span>
                    </span>
                    <Badge>Linked in file</Badge>
                  </button>
                </li>
              ))}
            </ul>
          ) : (
            <EmptyState title="No linked files">
              Folio currently shows only links written inside files.
            </EmptyState>
          ))}
      </div>
    </aside>
  );
}

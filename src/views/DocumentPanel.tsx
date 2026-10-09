import { ArrowLeft, FileText, Sparkles, X } from "lucide-react";
import { useId, useRef, useState, type KeyboardEvent } from "react";
import type { WorkspaceState } from "../app/useWorkspace";
import type { DocumentRecord } from "../domain/contracts";
import type { ViewId } from "../shell/navigation";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { EmptyState } from "../ui/EmptyState";
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
        <span className="document-icon" aria-hidden="true">
          <FileText size={24} />
        </span>
        <div className="document-heading">
          <h2 className="document-title" title={document.name}>
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
            aria-controls={`${id}-panel-${name}`}
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
                    <FileText size={20} aria-hidden="true" />
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

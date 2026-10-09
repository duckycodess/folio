import { useEffect, useMemo, useRef, useState } from "react";
import {
  chooseWorkspace,
  fixtureDocuments,
  nativeAvailable,
  readNativeDocument,
} from "./adapters/workspace";
import { discoverExplicitReferences, keywordSearch } from "./domain/discovery";
import type { DocumentRecord, WorkspaceInfo } from "./domain/contracts";

type Area = "Search" | "Organize" | "Summarize";

export default function App() {
  const [area, setArea] = useState<Area>("Search");
  const [documents, setDocuments] = useState(fixtureDocuments);
  const [workspace, setWorkspace] = useState<WorkspaceInfo | null>(null);
  const [selectedId, setSelectedId] = useState("projects/project-plan.md");
  const [query, setQuery] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [rename, setRename] = useState("");
  const [renamePreview, setRenamePreview] = useState<string | null>(null);
  const [assistantOpen, setAssistantOpen] = useState(false);
  const [instruction, setInstruction] = useState("");
  const [notice, setNotice] = useState("");
  const selectionRequest = useRef(0);
  const assistantDialog = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const dialog = assistantDialog.current;
    if (!dialog) return;
    if (assistantOpen && !dialog.open) dialog.showModal();
    if (!assistantOpen && dialog.open) dialog.close();
  }, [assistantOpen]);
  const selected = documents.find((document) => document.id === selectedId);
  const results = useMemo(
    () => keywordSearch(documents, query),
    [documents, query],
  );
  const relationships = useMemo(
    () => discoverExplicitReferences(documents),
    [documents],
  );
  const neighbors = useMemo(
    () =>
      [
        ...new Set(
          relationships
            .filter(
              (edge) =>
                edge.sourceId === selectedId || edge.targetId === selectedId,
            )
            .map((edge) =>
              edge.sourceId === selectedId ? edge.targetId : edge.sourceId,
            ),
        ),
      ]
        .map((id) => documents.find((document) => document.id === id)!)
        .filter(Boolean),
    [relationships, selectedId, documents],
  );

  async function selectDocument(document: DocumentRecord) {
    const request = ++selectionRequest.current;
    setSelectedId(document.id);
    setRename("");
    setRenamePreview(null);
    setNotice("");
    setError("");
    if (!workspace || document.content !== undefined) {
      setBusy(false);
      return;
    }
    setBusy(true);
    try {
      const read = await readNativeDocument(workspace.id, document);
      if (request !== selectionRequest.current) return;
      setDocuments((current) =>
        current.map((item) => (item.id === read.id ? read : item)),
      );
    } catch (cause) {
      if (request === selectionRequest.current) setError(String(cause));
    } finally {
      if (request === selectionRequest.current) setBusy(false);
    }
  }

  async function selectFolder() {
    const request = ++selectionRequest.current;
    setError("");
    setBusy(true);
    try {
      const chosen = await chooseWorkspace();
      if (request !== selectionRequest.current) return;
      if (!chosen) return;
      setWorkspace(chosen.info);
      setDocuments(chosen.documents);
      setSelectedId("");
      setQuery("");
      setRenamePreview(null);
      setNotice("");
    } catch (cause) {
      if (request === selectionRequest.current) setError(String(cause));
    } finally {
      if (request === selectionRequest.current) setBusy(false);
    }
  }

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <a
          className="brand"
          href="#"
          onClick={(event) => {
            event.preventDefault();
            setArea("Search");
          }}
        >
          <span className="brand-mark">f</span>folio
          <span className="brand-dot">.</span>
        </a>
        <p className="sidebar-caption">
          A little order.
          <br />A clearer picture.
        </p>
        <nav aria-label="Folio SOS">
          {(["Search", "Organize", "Summarize"] as Area[]).map(
            (item, index) => (
              <button
                key={item}
                className={`nav-item ${area === item ? "active" : ""}`}
                onClick={() => {
                  setArea(item);
                  setNotice("");
                }}
              >
                <span className="nav-symbol">{["⌕", "▤", "≡"][index]}</span>
                {item}
                <span className="nav-index">0{index + 1}</span>
              </button>
            ),
          )}
        </nav>
        <div className="workspace-section">
          <span className="eyebrow">WORKSPACE</span>
          <strong>{workspace ? "Selected folder" : "Demo documents"}</strong>
          <span>
            {documents.length} files ·{" "}
            {workspace ? "Native folder" : "Synthetic examples"}
          </span>
          <button
            className="outline-button"
            disabled={!nativeAvailable || busy}
            onClick={selectFolder}
          >
            Choose a folder
          </button>
          {!nativeAvailable && (
            <small>Folder access is available in the desktop app.</small>
          )}
        </div>
        <div className="sidebar-bottom">
          <span className="status-dot" /> On-device workspace
          <span className="version">Starter · v0.1</span>
        </div>
      </aside>

      <main className="main-pane">
        <header className="topbar">
          <span>YOUR FILES, CONNECTED</span>
          <button
            className="assistant-button"
            onClick={() => setAssistantOpen(true)}
          >
            Ask & Act <span>↗</span>
          </button>
        </header>
        <section className="page-heading">
          <div>
            <span className="eyebrow">SEARCH · ORGANIZE · SUMMARIZE</span>
            <h1>
              {area === "Search"
                ? "Find your way back."
                : area === "Organize"
                  ? "Give your files a little order."
                  : "Get the important parts."}
            </h1>
            <p>
              {area === "Search"
                ? "Explore documents, their contents, and the files they reference."
                : area === "Organize"
                  ? "Review filenames and destinations before making a change."
                  : "Read a document and connect a local model for source-grounded summaries."}
            </p>
          </div>
          <span className="privacy-pill">
            <span className="status-dot" /> Local files
          </span>
        </section>
        <div className="implementation-note">
          <strong>Starter preview</strong>
          <span>
            Keyword search is active. Semantic search, AI summaries, and
            save/undo are awaiting their adapters.
          </span>
        </div>
        {error && (
          <div role="alert" className="error-banner">
            {error}
          </div>
        )}
        <section className="search-bar">
          <span aria-hidden="true">⌕</span>
          <input
            aria-label="Search documents"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Find a file or passage… / Hanapin yung notes…"
          />
          <span className="search-mode">Keyword mode</span>
        </section>

        <div className="workspace-grid">
          <section className="file-panel" aria-label="Files">
            <div className="panel-heading">
              <h2>Documents</h2>
              <span>{results.length}</span>
            </div>
            <div className="file-list">
              {results.map(({ document }) => (
                <button
                  key={document.id}
                  className={`file-row ${selectedId === document.id ? "selected" : ""}`}
                  onClick={() => selectDocument(document)}
                >
                  <span className="file-icon">
                    {document.name.split(".").at(-1)?.toUpperCase()}
                  </span>
                  <span className="file-label">
                    <strong>{document.title}</strong>
                    <small>{document.relativePath}</small>
                  </span>
                  <span className="file-arrow">↗</span>
                </button>
              ))}
              {results.length === 0 && (
                <p className="empty-state">
                  No keyword matches. Try a word from the document.
                </p>
              )}
            </div>
          </section>

          <section className="detail-panel" aria-label="Selected document">
            {selected ? (
              <>
                <div className="document-heading">
                  <span className="eyebrow">
                    {selected.language === "fil"
                      ? "FILIPINO"
                      : selected.language === "mixed"
                        ? "TAGLISH"
                        : selected.language === "en"
                          ? "ENGLISH"
                          : "LANGUAGE NOT CLASSIFIED"}{" "}
                    · {selected.name.split(".").at(-1)?.toUpperCase()}
                  </span>
                  <h2>{selected.title}</h2>
                  <p>{selected.relativePath}</p>
                </div>
                {area === "Organize" && (
                  <div className="action-card">
                    <h3>Preview a filename</h3>
                    <p>
                      Enter a filename to review. The native apply engine is not
                      connected yet.
                    </p>
                    <label htmlFor="filename">New filename</label>
                    <div className="rename-control">
                      <input
                        id="filename"
                        value={rename}
                        onChange={(event) => {
                          setRename(event.target.value);
                          setRenamePreview(null);
                        }}
                        placeholder={selected.name}
                      />
                      <button
                        disabled={!rename.trim() || /[\\/]/.test(rename)}
                        onClick={() => setRenamePreview(rename.trim())}
                      >
                        Preview
                      </button>
                    </div>
                    {renamePreview && (
                      <div className="rename-preview">
                        <span>{selected.name}</span>
                        <span>→</span>
                        <strong>{renamePreview}</strong>
                        <small>Preview only · no file changed</small>
                      </div>
                    )}
                  </div>
                )}
                {area === "Summarize" && (
                  <div className="action-card">
                    <span className="eyebrow">SUMMARIZE</span>
                    <h3>A summary with its sources.</h3>
                    <p>
                      Connect the local generation adapter to enable English,
                      Filipino, and Taglish summaries. The original text is
                      available below.
                    </p>
                    <button
                      className="outline-button"
                      onClick={() =>
                        setNotice(
                          "Local generation is not connected. See docs/plan.md · Track T2.",
                        )
                      }
                    >
                      Check summary availability
                    </button>
                  </div>
                )}
                {notice && (
                  <p role="status" className="notice">
                    {notice}
                  </p>
                )}
                <div className="source-heading">
                  <h3>Source content</h3>
                  <span>
                    {busy
                      ? "Reading…"
                      : `${Math.ceil(selected.sizeBytes / 1024)} KB`}
                  </span>
                </div>
                <pre className="source-content">
                  {selected.content ??
                    (selected.name.toLowerCase().endsWith(".pdf")
                      ? "Text-based PDF extraction is the next indexing task."
                      : "Select this file to read its contents.")}
                </pre>
                <div className="related-heading">
                  <h3>Connected documents</h3>
                  <span>Explicit references</span>
                </div>
                {neighbors.length > 0 ? (
                  <>
                    <svg
                      className="reference-graph"
                      viewBox="0 0 600 170"
                      role="img"
                      aria-label={`${selected.name} has explicit links to ${neighbors.length} documents`}
                    >
                      <rect
                        x="200"
                        y="8"
                        width="200"
                        height="40"
                        rx="10"
                        className="graph-source"
                      />
                      <text x="300" y="33" textAnchor="middle">
                        {selected.name.slice(0, 25)}
                      </text>
                      {neighbors.slice(0, 3).map((document, index, visible) => {
                        const x = (600 / visible.length) * (index + 0.5);
                        return (
                          <g key={document.id}>
                            <path
                              d={`M300 48 C300 82 ${x} 82 ${x} 110`}
                              className="graph-edge"
                            />
                            <rect
                              x={x - 89}
                              y="110"
                              width="178"
                              height="40"
                              rx="10"
                              className="graph-target"
                            />
                            <text x={x} y="135" textAnchor="middle">
                              {document.name.slice(0, 22)}
                            </text>
                          </g>
                        );
                      })}
                    </svg>
                    <div className="related-list">
                      {neighbors.map((document) => (
                        <button
                          key={document.id}
                          onClick={() => selectDocument(document)}
                        >
                          <span>{document.name}</span>
                          <small>{document.relativePath}</small>
                          <span>↗</span>
                        </button>
                      ))}
                    </div>
                  </>
                ) : (
                  <p className="empty-state">
                    No explicit document links found in the currently loaded
                    text. Semantic connections will appear after indexing is
                    connected.
                  </p>
                )}
              </>
            ) : (
              <div className="empty-document">
                <h2>Select a document.</h2>
                <p>Read its source and discover its references.</p>
              </div>
            )}
          </section>
        </div>
        <footer className="page-footer">
          <span>
            {workspace
              ? workspace.rootPath
              : "Prepared English · Filipino · Taglish corpus"}
          </span>
          <span>Model Lab · no model loaded</span>
        </footer>
      </main>

      <dialog
        ref={assistantDialog}
        className="assistant-modal"
        aria-labelledby="assistant-title"
        onClose={() => setAssistantOpen(false)}
      >
        <div className="panel-heading">
          <span className="eyebrow">WORKFLOW C</span>
          <button
            aria-label="Close Ask and Act"
            onClick={() => setAssistantOpen(false)}
          >
            ×
          </button>
        </div>
        <h2 id="assistant-title">Ask & Act</h2>
        <p>
          Find files, understand their contents, then preview actions and their
          impact.
        </p>
        <label htmlFor="instruction">Your instruction</label>
        <textarea
          id="instruction"
          autoFocus
          value={instruction}
          onChange={(event) => setInstruction(event.target.value)}
          placeholder="Hanapin yung project plan at palitan ang deadline…"
        />
        <button
          className="primary-button"
          disabled={!instruction.trim()}
          onClick={() =>
            setNotice(
              "The local command interpreter and action engine are not connected. No files were changed.",
            )
          }
        >
          Check action availability
        </button>
        {notice && (
          <p role="status" className="notice">
            {notice}
          </p>
        )}
        <div className="modal-footnote">
          Find targets → Preview actions & impacts → Approve & Save
        </div>
      </dialog>
    </div>
  );
}

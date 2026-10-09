import { History, Maximize2, Paperclip, Send, Trash2, X } from "lucide-react";
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { PRACTICE_LABEL } from "../adapters/mockChat";
import { folderChoices } from "../app/fileActions";
import { matchSlashCommands, parseCommand } from "../app/commands";
import { requestForFile } from "../app/proposals";
import { useAskAct, type ConversationSummary } from "../app/useAskAct";
import type { RelationshipsState } from "../app/useRelationships";
import type { WorkspaceState } from "../app/useWorkspace";
import type { DocumentRecord, OperationProposal } from "../domain/contracts";
import type { ViewId } from "../shell/navigation";
import { useAnnounce } from "../ui/Announcer";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { trapTabWithin } from "../ui/Modal";
import { OlioSprite } from "../ui/OlioSprite";
import { TurnBody, type OpenFile } from "./AskTurns";
import { ChangeDialog } from "./ChangeDialog";

const GREETING_KEY = "folio.olioChat.greetingDismissed";

// Storage can be missing or throw; the greeting then shows again next time.
function greetingDismissed(): boolean {
  try {
    return window.localStorage.getItem(GREETING_KEY) === "1";
  } catch {
    return false;
  }
}

function rememberDismissed() {
  try {
    window.localStorage.setItem(GREETING_KEY, "1");
  } catch {
    // Dismissed for this session only.
  }
}

function HistoryList({
  history,
  onOpen,
  onNew,
}: {
  history: ConversationSummary[];
  onOpen: (id: string) => void;
  onNew: () => void;
}) {
  return (
    <div className="olio-chat-history">
      <Button variant="secondary" onClick={onNew}>
        New conversation
      </Button>
      {history.length === 0 ? (
        <p className="muted">No earlier conversations in this folder yet.</p>
      ) : (
        <ul className="olio-chat-history-list">
          {history.map((conversation) => (
            <li key={conversation.id}>
              <button
                type="button"
                className="olio-chat-history-item"
                onClick={() => onOpen(conversation.id)}
              >
                {conversation.title}
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/**
 * The floating Olio launcher and its compact chat, on every view except
 * Ask & Act (#66). Shares one conversation store with the full Ask & Act
 * page via `useAskAct`: there is no separate copy of turns to keep in sync.
 */
export function FloatingOlioChat({
  workspace,
  relations,
  view,
  onNavigate,
  onOpenFile,
  currentFile,
  localAiLabel,
}: {
  workspace: WorkspaceState;
  relations: RelationshipsState;
  view: ViewId;
  onNavigate: (view: ViewId) => void;
  onOpenFile: OpenFile;
  /** The file open in the reader, if any; offered as an attachable chip. */
  currentFile?: DocumentRecord;
  /** The shell's reading of the model store, the same one the sidebar shows. */
  localAiLabel: string;
}) {
  const ask = useAskAct(workspace);
  const announce = useAnnounce();
  const [open, setOpen] = useState(false);
  const [panel, setPanel] = useState<"chat" | "history">("chat");
  const [greeting, setGreeting] = useState(() => !greetingDismissed());
  const [text, setText] = useState("");
  const [attached, setAttached] = useState<DocumentRecord | null>(null);
  const [changing, setChanging] = useState<OperationProposal | null>(null);
  const launcherRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  // The running turn this chat saw start; only its end is announced, so
  // stored history and switching conversations announce nothing.
  const watching = useRef<number | null>(null);
  const headingId = useId();

  // Attaching a file only makes sense while that file is still open.
  useEffect(() => {
    if (attached && currentFile?.id !== attached.id) setAttached(null);
  }, [attached, currentFile]);

  const latest = ask.turns.at(-1);
  useEffect(() => {
    if (!latest) return;
    if (latest.status === "running") {
      watching.current = latest.id;
      return;
    }
    if (watching.current !== latest.id) return;
    watching.current = null;
    // On Ask & Act the page announces the same turn itself.
    if (view === "assistant") return;
    announce(
      latest.status === "cancelled"
        ? "Request cancelled."
        : latest.status === "failed"
          ? "The request didn't finish."
          : "Olio's reply is ready.",
    );
  }, [latest, announce, view]);

  useEffect(() => {
    if (!open) return;
    function onKeyDown(event: globalThis.KeyboardEvent) {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      // Escape inside a dialog over the chat (its change preview, or any
      // other modal) belongs to that dialog, not to the chat behind it.
      if (changing || !panelRef.current?.contains(event.target as Node)) return;
      event.preventDefault();
      close();
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, changing]);

  useEffect(() => {
    if (open) textareaRef.current?.focus();
  }, [open, panel]);

  if (view === "assistant") return null;

  function close() {
    setOpen(false);
    requestAnimationFrame(() => launcherRef.current?.focus());
  }

  function launch() {
    rememberDismissed();
    setGreeting(false);
    setOpen(true);
    setPanel("chat");
  }

  function send() {
    const typed = text.trim();
    if (!typed || ask.busy) return;
    const command = parseCommand(typed);
    if (command?.name === "organize") {
      onNavigate("organize");
      close();
    } else if (command?.name === "search") {
      // "/search" alone has nothing to look for; keep the text so the user
      // can add a query.
      if (!command.args) return;
      ask.find(command.args);
    } else if (command?.name === "summarize") {
      // Without an argument, summarize the attached or open file.
      const file = attached ?? currentFile;
      if (command.args) ask.ask(`Summarize ${command.args}`);
      else if (file)
        ask.ask(requestForFile("Summarize this file", file.relativePath), file);
      else ask.ask("Summarize this file");
    } else if (attached) {
      // The attached file binds the request: a change to any other file is
      // refused before a preview.
      ask.ask(requestForFile(typed, attached.relativePath), attached);
    } else {
      ask.ask(typed);
    }
    setText("");
    setAttached(null);
  }

  function onComposerKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      send();
    }
  }

  const folders = folderChoices(workspace.documents).filter(Boolean);
  const suggestions =
    text.startsWith("/") && !text.includes(" ")
      ? matchSlashCommands(text.slice(1))
      : [];
  const turns = [...ask.turns].reverse();

  return (
    <div className="olio-chat">
      {!open && (
        <div className="olio-chat-launcher">
          {greeting && (
            <p className="olio-chat-greeting">
              <strong>Hey, I&rsquo;m Olio.</strong> Ask about your files.
              <button
                type="button"
                className="icon-button olio-chat-greeting-dismiss"
                aria-label="Dismiss greeting"
                onClick={() => {
                  rememberDismissed();
                  setGreeting(false);
                }}
              >
                <X size={16} aria-hidden="true" />
              </button>
            </p>
          )}
          <button
            ref={launcherRef}
            type="button"
            className="olio-chat-launcher-button"
            aria-label="Ask Olio"
            onClick={launch}
          >
            <OlioSprite state="idle" size={150} />
          </button>
        </div>
      )}

      {open && (
        <div
          ref={panelRef}
          role="dialog"
          aria-labelledby={headingId}
          aria-modal="false"
          className={`olio-chat-panel${ask.busy ? " olio-chat-thinking" : ""}`}
          onKeyDown={trapTabWithin}
        >
          <header className="olio-chat-head">
            <OlioSprite state={ask.busy ? "thinking" : "idle"} size={64} />
            <div className="olio-chat-head-text">
              <h2 id={headingId} className="olio-chat-title">
                Olio
              </h2>
              <p className="olio-chat-status">{localAiLabel}</p>
            </div>
            <Button
              variant="ghost"
              icon={<History size={18} />}
              aria-pressed={panel === "history"}
              aria-label="History"
              onClick={() => setPanel(panel === "history" ? "chat" : "history")}
            />
            <Button
              variant="ghost"
              icon={<Maximize2 size={18} />}
              aria-label="Expand to Ask & Act"
              onClick={() => {
                onNavigate("assistant");
                close();
              }}
            />
            <Button
              variant="ghost"
              icon={<X size={18} />}
              onClick={close}
              aria-label="Close"
            />
          </header>

          {panel === "history" ? (
            <HistoryList
              history={ask.history}
              onOpen={(id) => {
                ask.openConversation(id);
                setPanel("chat");
              }}
              onNew={() => {
                ask.newConversation();
                setPanel("chat");
              }}
            />
          ) : (
            <>
              <div className="olio-chat-messages" aria-live="off">
                {turns.length === 0 ? (
                  <div className="olio-chat-welcome">
                    <h3 className="olio-chat-welcome-title">
                      Hey, how can I help?
                    </h3>
                    <p className="muted">
                      Find something by meaning, organize your files, or get a
                      summary. Nothing changes without your approval.
                    </p>
                    <button
                      type="button"
                      className="olio-chat-welcome-action"
                      onClick={() => {
                        setText("/search ");
                        textareaRef.current?.focus();
                      }}
                    >
                      Find a file
                    </button>
                    <button
                      type="button"
                      className="olio-chat-welcome-action"
                      onClick={() => {
                        onNavigate("organize");
                        close();
                      }}
                    >
                      Organize my files
                    </button>
                    <button
                      type="button"
                      className="olio-chat-welcome-action"
                      onClick={() => {
                        setText("/summarize ");
                        textareaRef.current?.focus();
                      }}
                    >
                      Summarize a file
                    </button>
                  </div>
                ) : (
                  turns.map((turn) => (
                    <article key={turn.id} className="olio-chat-turn">
                      <p className="olio-chat-request">
                        <Badge>{turn.action === "find" ? "Find" : "Ask"}</Badge>{" "}
                        <q>{turn.request}</q>
                      </p>
                      <TurnBody
                        turn={turn}
                        ask={ask}
                        workspace={workspace}
                        relations={relations}
                        onOpen={onOpenFile}
                        onRetry={() =>
                          turn.action === "find"
                            ? ask.find(turn.request)
                            : ask.ask(turn.request, turn.chosen)
                        }
                        onNavigate={onNavigate}
                        onPreviewChange={setChanging}
                      />
                    </article>
                  ))
                )}
              </div>

              {ask.desktop && folders.length > 0 && (
                <p className="olio-chat-scope muted">
                  Searching {ask.scope ? `in ${ask.scope}` : "the whole folder"}
                  .
                </p>
              )}

              {currentFile && (
                <div className="olio-chat-chips">
                  {attached?.id === currentFile.id ? (
                    <Badge>
                      Attached: {currentFile.name}{" "}
                      <button
                        type="button"
                        className="icon-button"
                        aria-label={`Remove ${currentFile.name} from this message`}
                        onClick={() => setAttached(null)}
                      >
                        <X size={12} aria-hidden="true" />
                      </button>
                    </Badge>
                  ) : (
                    <Button
                      variant="ghost"
                      icon={<Paperclip size={14} />}
                      onClick={() => setAttached(currentFile)}
                    >
                      Attach {currentFile.name}
                    </Button>
                  )}
                </div>
              )}

              <form
                className="olio-chat-composer"
                onSubmit={(event) => {
                  event.preventDefault();
                  send();
                }}
              >
                {suggestions.length > 0 && (
                  <ul className="olio-chat-suggestions">
                    {suggestions.map((command) => (
                      <li key={command.name}>
                        <button
                          type="button"
                          className="olio-chat-suggestion"
                          onClick={() => {
                            setText(`/${command.name} `);
                            textareaRef.current?.focus();
                          }}
                        >
                          <span>{command.usage}</span>
                          <span className="muted">{command.hint}</span>
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
                <label htmlFor="olio-chat-input" className="visually-hidden">
                  Message Olio
                </label>
                <textarea
                  id="olio-chat-input"
                  ref={textareaRef}
                  className="text-area olio-chat-input"
                  rows={2}
                  value={text}
                  placeholder="Ask Olio, or type / for commands"
                  onChange={(event) => setText(event.target.value)}
                  onKeyDown={onComposerKeyDown}
                />
                <Button
                  type="submit"
                  variant="primary"
                  icon={<Send size={16} />}
                  disabled={!text.trim() || ask.busy}
                >
                  <span className="visually-hidden">Send</span>
                </Button>
              </form>
              <p className="olio-chat-note muted">
                {ask.desktop
                  ? "Core work stays on this device."
                  : PRACTICE_LABEL}
              </p>
              {(ask.turns.length > 0 || ask.history.length > 0) && (
                <Button
                  variant="ghost"
                  icon={<Trash2 size={14} />}
                  onClick={() => {
                    if (
                      window.confirm(
                        "Delete all conversations kept on this device?",
                      )
                    )
                      ask.deleteAllConversations();
                  }}
                >
                  Delete all conversations
                </Button>
              )}
            </>
          )}
        </div>
      )}

      {changing && (
        <ChangeDialog
          proposal={changing}
          workspace={workspace}
          relations={relations}
          onClose={() => setChanging(null)}
        />
      )}
    </div>
  );
}

import { Check, Pencil, X } from "lucide-react";
import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { MAX_TITLE_LENGTH } from "../app/chatStore";
import type { ConversationSummary } from "../app/useAskAct";
import { Button } from "../ui/Button";

/**
 * A conversation's name with a Rename button. Renaming edits in place: Enter
 * or the check saves, Escape or the cross cancels, and a blank name goes back
 * to the automatic one (the first request).
 */
export function ConversationName({
  title,
  onRename,
  className,
  label,
}: {
  title: string;
  onRename: (title: string) => void;
  className?: string;
  /** Shown instead of the plain name, e.g. a button that opens it. */
  label?: ReactNode;
}) {
  const inputId = useId();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(title);
  const input = useRef<HTMLInputElement | null>(null);
  const renameButton = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    if (editing) input.current?.select();
  }, [editing]);

  function finish(save: boolean) {
    if (save) onRename(draft);
    setEditing(false);
    requestAnimationFrame(() => renameButton.current?.focus());
  }

  if (editing)
    return (
      <form
        className={`conversation-name conversation-name-editing${className ? ` ${className}` : ""}`}
        onSubmit={(event) => {
          event.preventDefault();
          finish(true);
        }}
      >
        <label className="visually-hidden" htmlFor={inputId}>
          Conversation name
        </label>
        <input
          id={inputId}
          ref={input}
          className="conversation-name-input"
          value={draft}
          maxLength={MAX_TITLE_LENGTH}
          placeholder="Name this conversation"
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              event.stopPropagation();
              finish(false);
            }
          }}
        />
        <Button
          type="submit"
          variant="ghost"
          icon={<Check size={16} />}
          aria-label="Save name"
        />
        <Button
          variant="ghost"
          icon={<X size={16} />}
          aria-label="Cancel renaming"
          onClick={() => finish(false)}
        />
      </form>
    );

  return (
    <div className={`conversation-name${className ? ` ${className}` : ""}`}>
      {label ?? (
        <span className="conversation-name-text" title={title}>
          {title}
        </span>
      )}
      <Button
        ref={renameButton}
        variant="ghost"
        icon={<Pencil size={14} />}
        aria-label={`Rename conversation: ${title}`}
        title="Rename"
        onClick={() => {
          setDraft(title);
          setEditing(true);
        }}
      />
    </div>
  );
}

/** This folder's other conversations, each one openable and renamable. */
export function ConversationHistory({
  history,
  onOpen,
  onNew,
  onRename,
}: {
  history: ConversationSummary[];
  onOpen: (id: string) => void;
  onNew: () => void;
  onRename: (id: string, title: string) => void;
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
              <ConversationName
                title={conversation.title}
                onRename={(title) => onRename(conversation.id, title)}
                label={
                  <button
                    type="button"
                    className="olio-chat-history-item"
                    onClick={() => onOpen(conversation.id)}
                  >
                    {conversation.title}
                  </button>
                }
              />
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

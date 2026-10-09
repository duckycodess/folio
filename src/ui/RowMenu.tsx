import { MoreHorizontal } from "lucide-react";
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";

export interface RowMenuItem {
  /** Stable name for code that picks items; the label is for people. */
  id: string;
  label: string;
  onSelect: () => void;
  /**
   * Why the action can't be used here. The item stays listed, so people learn
   * it exists, but it can't be chosen and the reason is read with it.
   */
  disabledReason?: string;
}

/**
 * A ⋯ button that opens a small menu of actions for one row. Arrow keys move
 * between items; Escape or Tab closes it and returns focus to the button.
 */
export function RowMenu({
  label,
  items,
  tabbable,
  openRequest = 0,
  onRequestOpened,
  restoreFocus,
}: {
  /** Spoken name of the button, e.g. "Actions for plan.md". */
  label: string;
  items: RowMenuItem[];
  /** Only the current row's menu is in the Tab order. */
  tabbable: boolean;
  /**
   * Opens the menu from elsewhere (Shift+F10 or a right-click on what the
   * menu acts on) each time the number goes up.
   */
  openRequest?: number;
  /**
   * Called once a request has opened the menu, so the caller can clear it and
   * a menu mounted later never reopens for an old request.
   */
  onRequestOpened?: () => void;
  /**
   * Where Escape sends focus instead of this ⋯ button, however the menu was
   * opened: the map node it acts on, so a second Escape deselects it.
   */
  restoreFocus?: () => void;
}) {
  const [open, setOpen] = useState(false);
  // Starts at 0, so a menu mounted by the request it should answer (a
  // right-click on a file that wasn't open yet) still opens.
  const seenRequest = useRef(0);
  const button = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const id = useId();
  const reasonId = useId();

  useEffect(() => {
    if (openRequest === seenRequest.current) return;
    seenRequest.current = openRequest;
    if (openRequest === 0) return;
    setOpen(true);
    onRequestOpened?.();
  }, [openRequest]);

  useEffect(() => {
    if (!open) return;
    menu.current?.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
    function onPointerDown(event: PointerEvent) {
      const target = event.target as Node;
      if (!menu.current?.contains(target) && !button.current?.contains(target))
        setOpen(false);
    }
    window.addEventListener("pointerdown", onPointerDown);
    return () => window.removeEventListener("pointerdown", onPointerDown);
  }, [open]);

  function close() {
    setOpen(false);
    if (restoreFocus) restoreFocus();
    else button.current?.focus();
  }

  function onMenuKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const entries = [
      ...event.currentTarget.querySelectorAll<HTMLElement>('[role="menuitem"]'),
    ];
    const index = entries.indexOf(document.activeElement as HTMLElement);
    const next =
      event.key === "ArrowDown"
        ? (index + 1) % entries.length
        : event.key === "ArrowUp"
          ? (index - 1 + entries.length) % entries.length
          : event.key === "Home"
            ? 0
            : event.key === "End"
              ? entries.length - 1
              : null;
    if (next !== null) {
      event.preventDefault();
      entries[next]?.focus();
    } else if (event.key === "Escape") {
      event.preventDefault();
      // Keep the shell's Escape (close the reader) from also running.
      event.stopPropagation();
      close();
    } else if (event.key === "Tab") {
      setOpen(false);
    }
  }

  return (
    <div className="row-menu">
      <button
        ref={button}
        type="button"
        className="icon-button row-menu-button"
        aria-label={label}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        tabIndex={tabbable ? 0 : -1}
        onClick={() => setOpen((value) => !value)}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown") {
            event.preventDefault();
            setOpen(true);
          }
        }}
      >
        <MoreHorizontal size={18} aria-hidden="true" />
      </button>
      {open && (
        <div
          ref={menu}
          id={id}
          role="menu"
          aria-label={label}
          className="row-menu-popup"
          onKeyDown={onMenuKeyDown}
        >
          {items.map((item) => (
            <button
              key={item.id}
              type="button"
              role="menuitem"
              tabIndex={-1}
              className="row-menu-item"
              aria-disabled={item.disabledReason ? true : undefined}
              aria-describedby={
                item.disabledReason ? `${reasonId}-${item.id}` : undefined
              }
              onClick={() => {
                if (item.disabledReason) return;
                setOpen(false);
                item.onSelect();
              }}
            >
              {item.label}
              {item.disabledReason && (
                <span id={`${reasonId}-${item.id}`} className="row-menu-reason">
                  {item.disabledReason}
                </span>
              )}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

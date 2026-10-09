import { MoreHorizontal } from "lucide-react";
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";

export interface RowMenuItem {
  label: string;
  onSelect: () => void;
}

/**
 * A ⋯ button that opens a small menu of actions for one row. Arrow keys move
 * between items; Escape or Tab closes it and returns focus to the button.
 */
export function RowMenu({
  label,
  items,
  tabbable,
}: {
  /** Spoken name of the button, e.g. "Actions for plan.md". */
  label: string;
  items: RowMenuItem[];
  /** Only the current row's menu is in the Tab order. */
  tabbable: boolean;
}) {
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const id = useId();

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
    button.current?.focus();
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
              key={item.label}
              type="button"
              role="menuitem"
              tabIndex={-1}
              className="row-menu-item"
              onClick={() => {
                setOpen(false);
                item.onSelect();
              }}
            >
              {item.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

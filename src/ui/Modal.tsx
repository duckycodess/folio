import { X } from "lucide-react";
import {
  useEffect,
  useId,
  useRef,
  type KeyboardEvent,
  type ReactNode,
} from "react";

const FOCUSABLE =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

interface ModalProps {
  open: boolean;
  title: string;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  /** Extra class on the dialog, e.g. `modal-wide`. */
  className?: string;
  /**
   * While true, Escape and the close button don't close the dialog: used while
   * a change is being saved, so its result can't be closed away unseen.
   */
  locked?: boolean;
}

/**
 * Native `<dialog>`: the page behind it is inert and Escape closes it. Tab and
 * Shift+Tab also wrap inside it, because the browser would otherwise let focus
 * leave for its own controls. Focus goes back to whatever opened the modal.
 */
export function Modal({
  open,
  title,
  onClose,
  children,
  footer,
  className,
  locked = false,
}: ModalProps) {
  const dialog = useRef<HTMLDialogElement>(null);
  const opener = useRef<Element | null>(null);
  const titleId = useId();

  function trapTab(event: KeyboardEvent<HTMLDialogElement>) {
    if (event.key !== "Tab") return;
    const items = [
      ...event.currentTarget.querySelectorAll<HTMLElement>(FOCUSABLE),
    ];
    if (!items.length) return;
    const first = items[0];
    const last = items[items.length - 1];
    const active = document.activeElement;
    if (
      event.shiftKey &&
      (active === first || !items.includes(active as HTMLElement))
    ) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && active === last) {
      event.preventDefault();
      first.focus();
    }
  }

  useEffect(() => {
    const element = dialog.current;
    if (!element) return;
    if (open && !element.open) {
      opener.current = document.activeElement;
      element.showModal();
    }
    if (!open && element.open) element.close();
  }, [open]);

  return (
    <dialog
      ref={dialog}
      className={`modal${className ? ` ${className}` : ""}`}
      aria-labelledby={titleId}
      onKeyDown={trapTab}
      onCancel={(event) => {
        if (locked) event.preventDefault();
      }}
      onClose={() => {
        onClose();
        if (opener.current instanceof HTMLElement) opener.current.focus();
      }}
    >
      <div className="modal-header">
        <h2 id={titleId} className="modal-title">
          {title}
        </h2>
        <button
          type="button"
          className="icon-button"
          aria-label="Close"
          disabled={locked}
          onClick={() => dialog.current?.close()}
        >
          <X size={18} aria-hidden="true" />
        </button>
      </div>
      <div className="modal-body">{children}</div>
      {footer && <div className="modal-footer">{footer}</div>}
    </dialog>
  );
}

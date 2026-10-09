import type { ReactNode } from "react";

interface ListRowProps {
  icon?: ReactNode;
  title: string;
  subtitle?: string;
  meta?: ReactNode;
  selected?: boolean;
  /** Only one row in the listbox is in the Tab order (roving tabindex). */
  tabbable?: boolean;
  dataId?: string;
  onSelect: () => void;
}

/** A selectable row inside an element with `role="listbox"`. */
export function ListRow({
  icon,
  title,
  subtitle,
  meta,
  selected = false,
  tabbable = selected,
  dataId,
  onSelect,
}: ListRowProps) {
  return (
    <button
      type="button"
      role="option"
      aria-selected={selected}
      tabIndex={tabbable ? 0 : -1}
      data-document-id={dataId}
      className={`list-row${selected ? " is-selected" : ""}`}
      onClick={onSelect}
      title={subtitle ? `${title}\n${subtitle}` : title}
    >
      {icon && (
        <span className="list-row-icon" aria-hidden="true">
          {icon}
        </span>
      )}
      <span className="list-row-text">
        <span className="list-row-title">{title}</span>
        {subtitle && <span className="list-row-subtitle">{subtitle}</span>}
      </span>
      {meta && <span className="list-row-meta">{meta}</span>}
    </button>
  );
}

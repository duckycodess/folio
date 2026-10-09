import type { ReactNode } from "react";

interface ListRowProps {
  icon?: ReactNode;
  title: string;
  subtitle?: string;
  /** Table cells after the title; laid out by the surrounding table. */
  cells?: ReactNode;
  meta?: ReactNode;
  /** Spoken name, when the visible text alone reads poorly (dashes, units). */
  label?: string;
  /** Hover text; defaults to the title and subtitle. */
  tooltip?: string;
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
  cells,
  meta,
  label,
  tooltip,
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
      aria-label={label}
      tabIndex={tabbable ? 0 : -1}
      data-document-id={dataId}
      className={`list-row${selected ? " is-selected" : ""}`}
      onClick={onSelect}
      title={tooltip ?? (subtitle ? `${title}\n${subtitle}` : title)}
    >
      <span className="list-row-main">
        {icon && (
          <span className="list-row-icon" aria-hidden="true">
            {icon}
          </span>
        )}
        <span className="list-row-text">
          <span className="list-row-title">{title}</span>
          {subtitle && <span className="list-row-subtitle">{subtitle}</span>}
        </span>
      </span>
      {cells && <span className="list-row-cells">{cells}</span>}
      {meta && <span className="list-row-meta">{meta}</span>}
    </button>
  );
}

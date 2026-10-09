import type { ReactNode, Ref } from "react";

interface PanelProps {
  title: string;
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
  /** Lets the caller move focus to the title, which can then take it. */
  titleRef?: Ref<HTMLHeadingElement>;
}

export function Panel({
  title,
  actions,
  children,
  className,
  titleRef,
}: PanelProps) {
  return (
    <section
      className={`panel${className ? ` ${className}` : ""}`}
      aria-label={title}
    >
      <div className="panel-header">
        <h2
          ref={titleRef}
          className="panel-title"
          tabIndex={titleRef ? -1 : undefined}
        >
          {title}
        </h2>
        {actions && <div className="panel-actions">{actions}</div>}
      </div>
      <div className="panel-body">{children}</div>
    </section>
  );
}

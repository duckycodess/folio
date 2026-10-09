import { CircleAlert, CircleCheck, Info } from "lucide-react";
import { useEffect, useRef, type ReactNode } from "react";
import { useAnnounce } from "./Announcer";

export type NoticeTone = "info" | "success" | "warning" | "danger";

interface NoticeProps {
  tone?: NoticeTone;
  children: ReactNode;
  action?: ReactNode;
}

const ICONS = {
  info: Info,
  success: CircleCheck,
  warning: CircleAlert,
  danger: CircleAlert,
} as const;

/**
 * Errors interrupt (`alert`, announced on insertion). Other notices are
 * announced politely through the page's shared live region.
 */
export function Notice({ tone = "info", children, action }: NoticeProps) {
  const Icon = ICONS[tone];
  const body = useRef<HTMLDivElement>(null);
  const announced = useRef("");
  const announce = useAnnounce();

  useEffect(() => {
    if (tone === "danger") return;
    const text = body.current?.textContent?.trim() ?? "";
    if (!text || text === announced.current) return;
    announced.current = text;
    announce(text);
  });

  return (
    <div
      className={`notice notice-${tone}`}
      role={tone === "danger" ? "alert" : undefined}
    >
      <Icon className="notice-icon" size={18} aria-hidden="true" />
      <div className="notice-body" ref={body}>
        {children}
      </div>
      {action && <div className="notice-action">{action}</div>}
    </div>
  );
}

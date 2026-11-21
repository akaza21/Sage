import { useState, type ReactNode } from "react";
import { CircleAlert, Info, TriangleAlert } from "./leanIcons";
import "./notice.css";

export type NoticeSeverity = "info" | "warning" | "blocking";

export interface NoticeProps {
  /** Problem kind and owning session, turn, or resource; never message text. */
  identity: string;
  severity: NoticeSeverity;
  message: ReactNode;
  action?: { label: string; onClick: () => void };
  details?: ReactNode;
  progress?: boolean;
  /** Historical failures are readable without replaying a live announcement. */
  announce?: boolean;
}

export function Notice(props: NoticeProps) {
  return <NoticeContent key={props.identity} {...props} />;
}

function NoticeContent({
  identity,
  severity,
  message,
  action,
  details,
  progress = false,
  announce = true,
}: NoticeProps) {
  const [expanded, setExpanded] = useState(false);
  const blocking = severity === "blocking" && announce;
  const [announcement, setAnnouncement] = useState<{
    message: ReactNode;
    live: boolean;
  } | null>(null);
  // Settle announcement state before committing changed copy to the tree.
  // Identity changes remount this bounded state; StrictMode replays preserve it.
  if (blocking && !announcement) setAnnouncement({ message, live: true });
  else if (announcement?.live && announcement.message !== message)
    setAnnouncement({ ...announcement, live: false });
  const Icon =
    severity === "blocking"
      ? CircleAlert
      : severity === "warning"
        ? TriangleAlert
        : Info;
  return (
    <div
      className={`notice notice--${severity}`}
      aria-live={
        progress
          ? "polite"
          : blocking && announcement?.live
            ? "assertive"
            : "off"
      }
      data-notice-identity={identity}
      role={
        announce && severity === "blocking"
          ? "alert"
          : progress
            ? "status"
            : undefined
      }
    >
      <Icon size={14} aria-hidden="true" />
      <div className="notice__body">
        <span className="sr-only">
          {severity === "blocking"
            ? "Error: "
            : severity === "warning"
              ? "Warning: "
              : ""}
        </span>
        <span>{message}</span>
        {action ? (
          <button
            type="button"
            className="notice__action"
            onClick={action.onClick}
          >
            {action.label}
          </button>
        ) : null}
        {details ? (
          <>
            <button
              type="button"
              className="notice__text-link notice__disclose"
              aria-expanded={expanded}
              onClick={() => setExpanded(!expanded)}
            >
              Details
            </button>
            {expanded ? (
              <div className="notice__details" aria-live="off">
                {details}
              </div>
            ) : null}
          </>
        ) : null}
      </div>
    </div>
  );
}

import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
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
  const region = useRef<HTMLDivElement>(null);
  const firstBlocking = useRef<{ message: ReactNode } | null>(null);
  // Silence later revisions of this problem before the accessibility tree
  // observes their DOM updates. Changing identity remounts this bounded state.
  useLayoutEffect(() => {
    let live = progress ? "polite" : "off";
    if (severity === "blocking" && announce) {
      // An unchanged StrictMode effect replay preserves the first announcement.
      if (
        !firstBlocking.current ||
        (firstBlocking.current.message === message &&
          region.current?.getAttribute("aria-live") === "assertive")
      )
        live = "assertive";
      firstBlocking.current ??= { message };
    }
    region.current?.setAttribute("aria-live", live);
  }, [announce, severity, message, progress]);
  const Icon =
    severity === "blocking"
      ? CircleAlert
      : severity === "warning"
        ? TriangleAlert
        : Info;
  return (
    <div
      ref={region}
      className={`notice notice--${severity}`}
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
          <div className="notice__details" aria-live="off">
            <button
              type="button"
              className="notice__disclose"
              aria-expanded={expanded}
              onClick={() => setExpanded(!expanded)}
            >
              Details
            </button>
            {expanded ? <div>{details}</div> : null}
          </div>
        ) : null}
      </div>
    </div>
  );
}

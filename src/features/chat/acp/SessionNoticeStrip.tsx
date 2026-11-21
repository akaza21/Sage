import { useState } from "react";

import type { AcpChatPage, SessionNotice } from "../../../domain/acpProjection";
import { Notice, type NoticeSeverity } from "../../../ui/Notice";
import { sessionNoticeProblems, turnFailureIdentity } from "./noticeOwnership";
import "./session-notice-strip.css";
import { SessionSurfacePanel } from "./SessionSurfacePanel";

const LEVEL_LABEL: Record<SessionNotice["level"], string> = {
  info: "notice",
  warning: "warning",
  error: "error",
};

const SEVERITY: Record<SessionNotice["level"], NoticeSeverity> = {
  info: "info",
  warning: "warning",
  error: "blocking",
};
const keyOf = (notice: SessionNotice) => notice.identity!;

export function SessionStrip({ page }: { page: AcpChatPage }) {
  return (
    <>
      {page.sessionNotices?.length ? (
        <SessionNoticeStrip
          session={JSON.stringify([page.chatId, page.currentSessionId])}
          notices={page.sessionNotices.filter(
            (notice) =>
              !page.turns.some(
                (turn) =>
                  turn.state === "failed" &&
                  notice.identity === turnFailureIdentity(turn),
              ),
          )}
        />
      ) : null}
      {page.sessionSurface ? (
        <SessionSurfacePanel
          surface={page.sessionSurface}
          chatId={page.chatId}
        />
      ) : null}
    </>
  );
}

interface Dismissed {
  session: string;
  keys: ReadonlySet<string>;
}

// Dismissals belong to one `session` and to the notices it still has, so the
// dismissed set never outgrows the list.
export function SessionNoticeStrip({
  session,
  notices: records,
}: {
  session: string;
  notices: readonly SessionNotice[];
}) {
  const notices = sessionNoticeProblems(records);
  const [dismissed, setDismissed] = useState<Dismissed>(() => ({
    session,
    keys: new Set(),
  }));
  const current = new Set(notices.map(keyOf));
  const kept =
    dismissed.session === session
      ? [...dismissed.keys].filter((key) => current.has(key))
      : [];
  if (dismissed.session !== session || kept.length !== dismissed.keys.size) {
    setDismissed({ session, keys: new Set(kept) });
  }
  const shown = notices.filter(
    (notice) =>
      (notice.diagnostic !== true || notice.level === "error") &&
      !kept.includes(keyOf(notice)),
  );
  if (shown.length === 0) return null;
  const dismiss = (keys: readonly string[]) =>
    setDismissed({ session, keys: new Set([...kept, ...keys]) });
  return (
    <section className="acp-session-notices" aria-label="Session notices">
      <ul>
        {shown.map((notice) => {
          const key = keyOf(notice);
          return (
            <li key={key} className="acp-session-notice">
              <Notice
                identity={JSON.stringify([session, key])}
                severity={SEVERITY[notice.level]}
                message={notice.text}
                details={notice.source ? `Source: ${notice.source}` : undefined}
              />
              <button
                type="button"
                className="acp-session-notice__dismiss"
                aria-label={`Dismiss ${LEVEL_LABEL[notice.level]}: ${notice.text}`}
                onClick={() => dismiss([key])}
              >
                ×
              </button>
            </li>
          );
        })}
      </ul>
      {shown.length > 1 ? (
        <button
          type="button"
          className="button button--compact acp-session-notices__clear"
          onClick={() => dismiss(shown.map(keyOf))}
        >
          Dismiss all
        </button>
      ) : null}
    </section>
  );
}

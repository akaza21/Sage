import { useState } from "react";

import {
  SESSION_NOTICES_MAX,
  type AcpChatPage,
  type SessionNotice,
} from "../../../domain/acpProjection";
import { Notice } from "../../../ui/Notice";
import "./session-notice-strip.css";
import { ConversationView } from "../MessageActionsTrigger";

export {
  ConversationView,
  MessageActionsTrigger,
  PromptActions,
} from "../MessageActionsTrigger";
export { ComposerChromeGate } from "./ComposerChromeGate";
const versionOf = (notice: SessionNotice) =>
  JSON.stringify([
    notice.identity,
    notice.level,
    notice.text,
    notice.diagnostic === true,
  ]);

export function SessionStrip({
  page,
  hasEarlier = false,
}: {
  page: AcpChatPage;
  hasEarlier?: boolean;
}) {
  return (
    <>
      {page.forkedFrom && !hasEarlier ? (
        <ConversationView
          view="ForkDivider"
          parentChatId={page.forkedFrom}
          forkTurnId={page.forkedFromTurnId}
          forkChatId={page.chatId}
        />
      ) : null}
      {page.sessionNotices?.length ? (
        <SessionNoticeStrip
          session={JSON.stringify([page.chatId, page.currentSessionId])}
          notices={page.sessionNotices}
        />
      ) : null}
    </>
  );
}

export function SessionSurface({ page }: { page: AcpChatPage }) {
  return (
    <ConversationView
      view="SessionSurfacePanel"
      surface={page.sessionSurface}
      chatId={page.chatId}
    />
  );
}

interface Dismissed {
  session: string;
  versions: readonly string[];
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
  const notices = records.slice(-SESSION_NOTICES_MAX);
  const [dismissed, setDismissed] = useState<Dismissed>(() => ({
    session,
    versions: [],
  }));
  const current = notices.map(versionOf);
  const kept =
    dismissed.session === session
      ? dismissed.versions.filter((version) => current.includes(version))
      : [];
  if (
    dismissed.session !== session ||
    kept.length !== dismissed.versions.length
  ) {
    setDismissed({ session, versions: kept });
  }
  const shown = notices.filter(
    (notice) =>
      (notice.diagnostic !== true || notice.level === "error") &&
      !kept.includes(versionOf(notice)),
  );
  if (shown.length === 0) return null;
  const dismiss = (records: readonly SessionNotice[]) =>
    setDismissed({
      session,
      versions: [...kept, ...records.map(versionOf)],
    });
  return (
    <section className="acp-session-notices" aria-label="Session notices">
      <ul>
        {shown.map((notice) => {
          const key = notice.identity;
          return (
            <li key={key} className="acp-session-notice">
              <Notice
                identity={JSON.stringify([session, key])}
                severity={notice.level === "error" ? "blocking" : notice.level}
                message={notice.text}
                details={notice.source ? `Source: ${notice.source}` : undefined}
                // Saved notices replay on every mount; only errors interrupt, as before.
                announce={notice.level === "error"}
              />
              <button
                type="button"
                className="acp-session-notice__dismiss"
                aria-label={`Dismiss ${notice.level === "info" ? "notice" : notice.level}: ${notice.text}`}
                onClick={() => dismiss([notice])}
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
          className="notice__text-link acp-session-notices__clear"
          onClick={() => dismiss(shown)}
        >
          Dismiss all
        </button>
      ) : null}
    </section>
  );
}

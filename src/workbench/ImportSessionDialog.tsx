import { useEffect, useId, useState } from "react";
import {
  commitIdentityChatResult,
  createChatTransitionContext,
} from "../app/chatTransitions";
import { currentProjectLayout } from "../app/projectProjection";
import { useWorkbenchStore } from "../app/store";
import { errorMessage } from "../domain/errors";
import { dayTimeLabel } from "../features/chat/chatTimestamp";
import {
  importNativeSession,
  listNativeSessions,
  type NativeSessionSummary,
} from "../services/nativeSessionImport";
import { ModalDialog } from "../ui/ModalDialog";

/** Opens a session from the active Project's own Codex CLI history as a new Chat. */
async function openSession(session: NativeSessionSummary) {
  const { setState: set, getState: get } = useWorkbenchStore;
  const layout = currentProjectLayout(get());
  if (!layout || get().projectTransitioning)
    return "Wait for the current Project operation.";
  const context = createChatTransitionContext(set, get);
  set({ projectTransitioning: true, error: null });
  const committed = await commitIdentityChatResult({
    set,
    get,
    applyProjection: context.applyProjection,
    loadAuthoritativeProjection: context.loadAuthoritativeProjection,
    failedMessage: "The session did not open.",
    invoke: () =>
      importNativeSession({
        currentLayout: layout,
        threadId: session.threadId,
        title: session.title,
        expectedTransitionEpoch: get().nativeTransitionEpoch,
      }),
  });
  if (committed) return null;
  const reason = get().error;
  set({ error: null });
  return reason || "Try again.";
}

export function ImportSessionDialog({
  projectId,
  onClose,
}: {
  projectId: string;
  onClose: () => void;
}) {
  const title = useId();
  const [sessions, setSessions] = useState<NativeSessionSummary[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState<string | null>(null);
  // The dialog lists one Project's sessions; it is mounted per opening.
  const fetchPage = (after: string | null) =>
    listNativeSessions(projectId, after)
      .then((page) => {
        setSessions((shown) => (after ? shown : []).concat(page.sessions));
        setCursor(page.nextCursor);
        setError(null);
      })
      .catch((reason: unknown) =>
        setError(
          `${errorMessage(reason)} Sessions are listed through a running Codex chat in this project.`,
        ),
      )
      .finally(() => setBusy(false));
  // eslint-disable-next-line react-hooks/exhaustive-deps -- first page once per mount
  useEffect(() => void fetchPage(null), []);
  return (
    <ModalDialog
      className="dirty-dialog dialog--scrim"
      labelledBy={title}
      onDismiss={() => !busy && onClose()}
    >
      <h2 id={title}>Open a Codex CLI session</h2>
      <div aria-busy={busy}>
        {sessions.map((session) => (
          <button
            key={session.threadId}
            type="button"
            className="chat-search-result"
            disabled={busy}
            onClick={async () => {
              setBusy(true);
              const reason = await openSession(session);
              setBusy(false);
              if (reason === null) onClose();
              else setError(reason);
            }}
          >
            <strong>{session.title}</strong>
            <span>{dayTimeLabel(session.updatedAt * 1000)}</span>
          </button>
        ))}
        {cursor ? (
          <button
            type="button"
            className="chat-search-dialog__more"
            disabled={busy}
            onClick={() => {
              setBusy(true);
              void fetchPage(cursor);
            }}
          >
            Load more
          </button>
        ) : null}
      </div>
      {!busy && !error && sessions.length === 0 ? (
        <p>No Codex CLI sessions for this project.</p>
      ) : null}
      {error ? <p role="alert">{error}</p> : null}
    </ModalDialog>
  );
}

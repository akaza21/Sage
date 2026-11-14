import { useEffect, useId, useRef, useState } from "react";
import {
  commitIdentityChatResult,
  createChatTransitionContext,
} from "../app/chatTransitions";
import { currentProjectLayout } from "../app/projectProjection";
import { useWorkbenchStore } from "../app/store";
import { errorMessage } from "../domain/errors";
import { dayTimeLabel } from "../features/chat/chatTimestamp";
import {
  canListMoreSessions,
  importNativeSession,
  listNativeSessions,
  type ImportLister,
  type NativeSessionSummary,
} from "../services/nativeSessionImport";
import { ModalDialog } from "../ui/ModalDialog";

async function openSession(
  session: NativeSessionSummary,
  lister: ImportLister,
) {
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
        lister,
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

export function ImportSessionDialog() {
  const [projectId] = useState(
    () => useWorkbenchStore.getState().project?.id ?? "",
  );
  const [closed, setClosed] = useState(false);
  const live = useRef(true);
  const onClose = () => setClosed(true);
  const title = useId();
  const [sessions, setSessions] = useState<NativeSessionSummary[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [lister, setLister] = useState<ImportLister | null>(null);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const fetchPage = (after: string | null) =>
    listNativeSessions(projectId, after, lister)
      .then((page) => {
        if (!live.current) return;
        setSessions((shown) => (after ? shown : []).concat(page.sessions));
        setCursor(page.nextCursor);
        setLister(page.lister);
        setError(null);
      })
      .catch((reason: unknown) => {
        if (live.current)
          setError(
            `${errorMessage(reason)} Sessions are listed through a running Codex chat in this project.`,
          );
      })
      .finally(() => live.current && setBusy(false));
  useEffect(() => {
    live.current = true;
    void fetchPage(null);
    return () => {
      live.current = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- first page once per mount
  }, []);
  if (closed) return null;
  return (
    <ModalDialog
      className="dirty-dialog dialog--scrim"
      labelledBy={title}
      onDismiss={() => {
        live.current = false;
        onClose();
      }}
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
              const reason = await openSession(session, lister!);
              if (!live.current) return;
              setBusy(false);
              if (reason === null) onClose();
              else setError(reason);
            }}
          >
            <strong>{session.title}</strong>
            <span>{dayTimeLabel(session.updatedAt * 1000)}</span>
          </button>
        ))}
        {cursor && canListMoreSessions(sessions.length) ? (
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
        ) : cursor ? (
          <p>Only the most recently active sessions are listed.</p>
        ) : null}
      </div>
      {!busy && !error && sessions.length === 0 ? (
        <p>No Codex CLI sessions for this project.</p>
      ) : null}
      {error ? <p role="alert">{error}</p> : null}
    </ModalDialog>
  );
}

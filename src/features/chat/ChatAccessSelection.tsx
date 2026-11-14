import {
  lazy,
  Suspense,
  useEffect,
  useLayoutEffect,
  useRef,
  type ReactNode,
  type RefObject,
} from "react";
import {
  loadChatAccess,
  useChatAccessStore,
} from "../../app/sessionAccessActions";
import { useSessionAccessModeStore } from "../../app/sessionAccessModeStore";
import type { SessionAccessModeCapability } from "../../domain/sessionAccessMode";
import {
  CHAT_ACCESS_UNAVAILABLE,
  chatAccessCandidates,
  chatAccessEffective,
  chatAccessUnavailable,
  heldChatAccessCandidates,
} from "./chatAccessMode";
import { CHAT_ACCESS_ICONS } from "./chatAccessIcons";
import type {
  AccessView,
  ComposerAccessControlProps,
} from "./ComposerAccessControl";
import { ChatAccessDraftSelection } from "./ChatAccessDraftSelection";
import { ComposerMenuLoading } from "./ComposerMenuLoading";
const AccessMenu = lazy(() => import("./ComposerAccessMenu"));

interface AccessSelectionProps {
  bindingId: string;
  capabilities: readonly SessionAccessModeCapability[] | undefined;
  error: string | null;
  children: (view: AccessView) => ReactNode;
}

/** The access menu's anchor, as the composer's trigger holds it. */
export interface AccessMenuAnchor {
  id: string;
  triggerRef: RefObject<HTMLButtonElement | null>;
  open: boolean;
  onDismiss: () => void;
}

/**
 * Hands the composer's trigger what it shows, when that changes. The trigger
 * stays the composer's own, so it is never remounted as this chunk loads.
 */
function ReportView({
  view,
  onView,
}: {
  view: AccessView;
  onView: (view: AccessView | null) => void;
}) {
  const latest = useRef(view);
  const shown = [view.effective, view.tone]
    .concat(view.candidates.map(({ id, label }) => id + label))
    .join("\n");
  // The newest view is kept first, so a report always hands it over; a
  // report is made only when what the trigger shows has changed.
  useLayoutEffect(() => {
    latest.current = view;
  });
  useLayoutEffect(() => onView(latest.current), [shown, onView]);
  useLayoutEffect(() => () => onView(null), [onView]);
  return null;
}

/**
 * A chat's own access, and its menu: its live session's, the one its next
 * session runs under while none is live, or before its first session the
 * access drafted for it. Loaded on demand, so the chat path carries none of
 * it.
 */
export default function ChatAccessSelection({
  bindingId,
  capabilities,
  sessionConfig: session,
  chatId,
  heldSessionId,
  id,
  triggerRef,
  open,
  onDismiss,
  onView,
}: ComposerAccessControlProps &
  AccessMenuAnchor & {
    onView: (view: AccessView | null) => void;
  }) {
  const props = { bindingId, capabilities };
  // The harness defaults' own load error shows with the chat's access.
  const error = useSessionAccessModeStore((state) => state.error);
  const withMenu = (view: AccessView) => (
    <>
      <ReportView view={view} onView={onView} />
      {view.candidates.length > 0 && view.error ? (
        <span
          className="select-control__reason composer-access-error"
          role="alert"
        >
          {view.error}
        </span>
      ) : null}
      {view.candidates.length > 0 && open ? (
        <Suspense
          fallback={
            <ComposerMenuLoading
              triggerRef={triggerRef}
              onDismiss={onDismiss}
            />
          }
        >
          <AccessMenu
            key={bindingId + view.sessionId}
            id={id}
            triggerRef={triggerRef}
            candidates={view.candidates}
            effective={view.effective}
            onDismiss={onDismiss}
            sessionId={view.sessionId}
            onDraft={view.onDraft}
          />
        </Suspense>
      ) : null}
    </>
  );
  if (session && !session.removed) {
    const advertised =
      session.modes?.availableModes.map((mode) => mode.id) ?? null;
    return (
      <OwnChatAccess
        sessionId={session.sageAgentSessionId}
        candidates={chatAccessCandidates(capabilities, advertised)}
        advertised={advertised}
        error={error}
        {...props}
      >
        {withMenu}
      </OwnChatAccess>
    );
  }
  // Without a live session, a chat that has had one shows and changes the
  // access its next session runs under; one that never has drafts it.
  return !chatId && heldSessionId ? (
    <OwnChatAccess
      sessionId={heldSessionId}
      candidates={heldChatAccessCandidates(capabilities)}
      advertised={null}
      error={error}
      {...props}
    >
      {withMenu}
    </OwnChatAccess>
  ) : (
    <ChatAccessDraftSelection chatId={chatId} error={error} {...props}>
      {withMenu}
    </ChatAccessDraftSelection>
  );
}

/**
 * A chat's own access as `sessionId` holds it: its own mode when it has one
 * among `candidates`, else its harness default. A mode of its own the
 * harness cannot carry now is reported, never replaced quietly.
 */
function OwnChatAccess({
  bindingId,
  capabilities,
  sessionId,
  candidates,
  advertised,
  error,
  children,
}: AccessSelectionProps & {
  sessionId: string;
  candidates: readonly SessionAccessModeCapability[];
  advertised: readonly string[] | null;
}) {
  const stored = useSessionAccessModeStore((state) => state.modes[bindingId]);
  const chat = useChatAccessStore((state) => state.modes[sessionId]);
  const chatError = useChatAccessStore((state) => state.error);
  const unread = chat === undefined;
  useEffect(() => {
    if (unread) void loadChatAccess(sessionId);
  }, [sessionId, unread]);
  const effective = chatAccessEffective(stored, candidates, chat);
  const unavailable = chatAccessUnavailable(capabilities, advertised, chat);
  return children({
    candidates,
    effective,
    error: error ?? (unavailable ? CHAT_ACCESS_UNAVAILABLE : chatError),
    sessionId,
    icon: effective ? CHAT_ACCESS_ICONS[effective] : undefined,
    tone:
      effective === "fullAccess" ? " composer__access-label--full" : undefined,
  });
}

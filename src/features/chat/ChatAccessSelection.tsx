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
import type { AcpSessionConfig } from "../../domain/acpSessionConfig";
import {
  sessionAccessModeAvailability,
  sessionAccessModeCandidates,
  type SessionAccessMode,
  type SessionAccessModeCapability,
} from "../../domain/sessionAccessMode";
import { CHAT_ACCESS_ICONS } from "./chatAccessIcons";
import type {
  AccessView,
  ComposerAccessControlProps,
} from "./ComposerAccessControl";
import { ChatAccessDraftSelection } from "./ChatAccessDraftSelection";
import { ComposerMenuLoading } from "./ComposerMenuLoading";
const AccessMenu = lazy(() => import("./ComposerAccessMenu"));

/**
 * The modes a live chat can take. A mode carried by a harness session mode
 * is offered only while the session advertises it (`advertised`, null until
 * reported), and Full access only once the session reports its modes.
 */
export function chatAccessCandidates(
  capabilities: readonly SessionAccessModeCapability[] | undefined,
  advertised: readonly string[] | null,
) {
  return sessionAccessModeCandidates(capabilities, advertised !== null).filter(
    (capability) =>
      capability.sessionModeId === undefined ||
      advertised === null ||
      advertised.includes(capability.sessionModeId),
  );
}

/** The mode the chat's next prompt runs under: its own, else the default. */
export function chatAccessEffective(
  stored: SessionAccessMode | undefined,
  candidates: readonly SessionAccessModeCapability[],
  chat: SessionAccessMode | null | undefined,
): SessionAccessMode | null {
  return (
    candidates.find((candidate) => candidate.id === chat)?.id ??
    sessionAccessModeAvailability(stored, candidates).effective
  );
}

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
 * A chat's own access, and its menu: its live session's, or before the
 * session exists the access drafted for it. Loaded on demand, so the chat
 * path carries none of it.
 */
export default function ChatAccessSelection({
  bindingId,
  capabilities,
  sessionConfig: session,
  chatId,
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
  // A removed session is past; its chat drafts like one not yet started.
  return session && !session.removed ? (
    <LiveChatAccess session={session} error={error} {...props}>
      {withMenu}
    </LiveChatAccess>
  ) : (
    <ChatAccessDraftSelection chatId={chatId} error={error} {...props}>
      {withMenu}
    </ChatAccessDraftSelection>
  );
}

/**
 * A live chat's access: its own mode when it has one the session offers,
 * else its harness default.
 */
function LiveChatAccess({
  bindingId,
  capabilities,
  session,
  error,
  children,
}: AccessSelectionProps & { session: AcpSessionConfig }) {
  const sessionId = session.sageAgentSessionId;
  const stored = useSessionAccessModeStore((state) => state.modes[bindingId]);
  const chat = useChatAccessStore((state) => state.modes[sessionId]);
  const chatError = useChatAccessStore((state) => state.error);
  const unread = chat === undefined;
  useEffect(() => {
    if (unread) void loadChatAccess(sessionId);
  }, [sessionId, unread]);
  const candidates = chatAccessCandidates(
    capabilities,
    session.modes?.availableModes.map((mode) => mode.id) ?? null,
  );
  const effective = chatAccessEffective(stored, candidates, chat);
  return children({
    candidates,
    effective,
    error: error ?? chatError,
    sessionId,
    icon: effective ? CHAT_ACCESS_ICONS[effective] : undefined,
    tone:
      effective === "fullAccess" ? " composer__access-label--full" : undefined,
  });
}

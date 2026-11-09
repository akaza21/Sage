import {
  lazy,
  Suspense,
  useEffect,
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
import type { AccessView } from "./ComposerAccessControl";
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
 * A chat's own access, and its menu: its live session's, or before the
 * session exists the access drafted for it. Loaded on demand, so the chat
 * path carries none of it.
 */
export default function ChatAccessSelection({
  session,
  chatId,
  anchor,
  children,
  ...props
}: AccessSelectionProps & {
  session?: AcpSessionConfig | null;
  chatId?: string;
  anchor: AccessMenuAnchor;
}) {
  const withMenu = (view: AccessView) =>
    view.candidates.length > 0 ? (
      <>
        {children(view)}
        {view.error ? (
          <span
            className="select-control__reason composer-access-error"
            role="alert"
          >
            {view.error}
          </span>
        ) : null}
        {anchor.open ? (
          <Suspense
            fallback={
              <ComposerMenuLoading
                triggerRef={anchor.triggerRef}
                onDismiss={anchor.onDismiss}
              />
            }
          >
            <AccessMenu
              key={props.bindingId + view.sessionId}
              id={anchor.id}
              triggerRef={anchor.triggerRef}
              candidates={view.candidates}
              effective={view.effective}
              onDismiss={anchor.onDismiss}
              sessionId={view.sessionId}
              onDraft={view.onDraft}
            />
          </Suspense>
        ) : null}
      </>
    ) : null;
  // A removed session is past; its chat drafts like one not yet started.
  return session && !session.removed ? (
    <LiveChatAccess session={session} {...props}>
      {withMenu}
    </LiveChatAccess>
  ) : (
    <ChatAccessDraftSelection chatId={chatId} {...props}>
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

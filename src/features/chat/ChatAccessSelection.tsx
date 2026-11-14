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
import type { SessionAccessModeCapability } from "../../domain/sessionAccessMode";
import { chatAccessCandidates, chatAccessEffective } from "./chatAccessMode";
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

export interface AccessMenuAnchor {
  id: string;
  triggerRef: RefObject<HTMLButtonElement | null>;
  open: boolean;
  onDismiss: () => void;
}

// The trigger stays the composer's own, so this chunk's load never remounts it.
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
  // Value-based reports must receive the latest view.
  useLayoutEffect(() => {
    latest.current = view;
  });
  useLayoutEffect(() => onView(latest.current), [shown, onView]);
  useLayoutEffect(() => () => onView(null), [onView]);
  return null;
}

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

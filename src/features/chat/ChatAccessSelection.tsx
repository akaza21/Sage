import { lazy, Suspense, useLayoutEffect, useRef, type RefObject } from "react";
import { useSessionAccessModeStore } from "../../app/sessionAccessModeStore";
import { ChatAccessView } from "./ChatAccessView";
import type {
  AccessView,
  ComposerAccessControlProps,
} from "./ComposerAccessControl";
import { ComposerMenuLoading } from "./ComposerMenuLoading";
const AccessMenu = lazy(() => import("./ComposerAccessMenu"));

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
  harnessName,
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
            harnessName={harnessName}
            onDraft={view.onDraft}
          />
        </Suspense>
      ) : null}
    </>
  );
  return (
    <ChatAccessView
      sessionConfig={session}
      chatId={chatId}
      error={error}
      {...props}
    >
      {withMenu}
    </ChatAccessView>
  );
}

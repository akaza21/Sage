import { useRef } from "react";
import { useWorkbenchStore } from "../app/store";
import { AgentIconRail } from "./AgentIconRail";
import { ChatSearchGate } from "./LeftSidebar";
import { SquarePen } from "../ui/leanIcons";

/** The rail alone, while the toggle hides the panel; a rail item brings the
 *  panel back on that item's list. */
export function AgentCollapsedNavigation({
  inert,
  onOpenSettings,
  onExpand,
  railRef,
}: {
  inert: boolean;
  onOpenSettings: () => void;
  onExpand: () => void;
  railRef?: (node: HTMLElement | null) => void;
}) {
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  return (
    <aside
      ref={railRef}
      className="project-sidebar"
      aria-label="Projects and chats"
      inert={inert}
      aria-hidden={inert || undefined}
    >
      <ChatSearchGate triggerRef={triggerRef} />
      <AgentIconRail
        inert={inert}
        onSettings={onOpenSettings}
        onShow={onExpand}
      />
    </aside>
  );
}

export function AgentNewChatButton() {
  const transitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  return (
    <button
      className="agent-task-header__pencil"
      type="button"
      aria-label="New chat"
      title="New chat"
      disabled={transitioning}
      onClick={() => void useWorkbenchStore.getState().openNewChatDraft(null)}
    >
      <SquarePen size={15} aria-hidden="true" />
    </button>
  );
}

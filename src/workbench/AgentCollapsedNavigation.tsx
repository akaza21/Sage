import { useRef } from "react";
import { useWorkbenchStore } from "../app/store";
import { requestChatSearchInFolder } from "../features/chatSearch/chatSearchScope";
import { AgentPrimaryNavigation } from "./AgentRailOptionsMenu";
import { ChatSearchGate } from "./LeftSidebar";
import { agentRailSectionSelectors } from "./agentRailNavigation";
import { SquarePen } from "../ui/leanIcons";

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
  const project = useWorkbenchStore((state) => state.project);
  const transitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
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
      <AgentPrimaryNavigation
        onSearch={() => {
          if (project) requestChatSearchInFolder(project.id);
        }}
        onSettings={onOpenSettings}
        searchDisabled={!project || transitioning}
        onNavigate={(target) => {
          onExpand();
          requestAnimationFrame(() => {
            const section = document
              .querySelector(".project-sidebar")
              ?.querySelector<HTMLElement>(agentRailSectionSelectors[target]);
            const disclosure = section?.querySelector<HTMLButtonElement>(
              ".agent-rail-section__disclosure",
            );
            if (disclosure?.getAttribute("aria-expanded") === "false")
              disclosure.click();
            section?.scrollIntoView?.();
            (
              disclosure ??
              section?.querySelector<HTMLElement>(".agent-rail-section__label")
            )?.focus();
          });
        }}
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
      onClick={() => void useWorkbenchStore.getState().openNewChatDraft()}
    >
      <SquarePen size={15} aria-hidden="true" />
    </button>
  );
}

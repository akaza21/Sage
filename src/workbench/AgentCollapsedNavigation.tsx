import { useRef } from "react";
import { useWorkbenchStore } from "../app/store";
import { requestChatSearchInFolder } from "../features/chatSearch/chatSearchScope";
import { AgentPrimaryNavigation } from "./AgentRailFilterMenu";
import { ChatSearchGate } from "./LeftSidebar";
import { agentRailSectionSelectors } from "./agentRailNavigation";

export { AgentNewChatButton } from "./AgentNewChatButton";

export function AgentCollapsedNavigation({
  inert,
  onOpenSettings,
  onExpand,
}: {
  inert: boolean;
  onOpenSettings: () => void;
  onExpand: () => void;
}) {
  const project = useWorkbenchStore((state) => state.project);
  const transitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const createChat = useWorkbenchStore((state) => state.createChat);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  return (
    <aside
      className="project-sidebar"
      aria-label="Projects and chats"
      inert={inert}
      aria-hidden={inert || undefined}
    >
      <ChatSearchGate triggerRef={triggerRef} />
      <AgentPrimaryNavigation
        onNewChat={() => void createChat()}
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
            disclosure?.focus();
          });
        }}
      />
    </aside>
  );
}

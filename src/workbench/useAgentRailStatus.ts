import { useEffect } from "react";
import { useWorkbenchStore } from "../app/store";
import { chatAttentionStore } from "../app/chatAttentionStore";
import { isTauriRuntime } from "../ports/desktopRuntime";
import { groupAgentRailProjects } from "./agentChatRail";

export function useAgentRailStatus() {
  const projectId = useWorkbenchStore((state) => state.project?.id);
  const projects = useWorkbenchStore((state) => state.projects);
  const tabs = useWorkbenchStore((state) => state.tabs);
  const transitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const groups = groupAgentRailProjects({
    projects,
    currentProjectId: projectId ?? null,
    currentChats: tabs.filter((tab) => tab.kind === "chat"),
  });
  const members = JSON.stringify(
    groups.map((group) => ({
      projectId: group.id,
      chatIds: group.chats.map((chat) => chat.id),
    })),
  );
  useEffect(() => {
    if (!isTauriRuntime() || transitioning) return;
    void chatAttentionStore.connect(JSON.parse(members));
    const refresh = () => {
      void chatAttentionStore.refresh();
    };
    const visible = () => {
      if (document.visibilityState === "visible") refresh();
    };
    window.addEventListener("focus", refresh);
    window.addEventListener("online", refresh);
    document.addEventListener("visibilitychange", visible);
    return () => {
      window.removeEventListener("focus", refresh);
      window.removeEventListener("online", refresh);
      document.removeEventListener("visibilitychange", visible);
      chatAttentionStore.disconnect();
    };
  }, [members, transitioning]);
}

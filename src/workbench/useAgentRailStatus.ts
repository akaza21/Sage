import { useEffect } from "react";
import { useWorkbenchStore } from "../app/store";
import type {
  ProjectGeneration,
  SageChatId,
  SageProjectId,
} from "../domain/acp";
import { isTauriRuntime } from "../ports/desktopRuntime";
import { acpDesktop } from "../services/acpDesktop";
import { mirrorAgentRailProjection } from "./useAgentRailProjection";

export function useAgentRailStatus() {
  const projectId = useWorkbenchStore((state) => state.project?.id);
  const generation = useWorkbenchStore((state) => state.projectGeneration);
  const epoch = useWorkbenchStore((state) => state.nativeTransitionEpoch);
  const transitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  useEffect(() => {
    if (!isTauriRuntime() || !projectId || transitioning) return;
    let disposed = false;
    let pending = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let unlisten: (() => void) | undefined;
    const dirty = new Set<string>();
    const current = () => {
      const state = useWorkbenchStore.getState();
      return (
        !disposed &&
        !state.projectTransitioning &&
        state.project?.id === projectId &&
        state.projectGeneration === generation &&
        state.nativeTransitionEpoch === epoch
      );
    };
    const refresh = async () => {
      if (!current() || pending) return;
      pending = true;
      const chatId = dirty.values().next().value;
      if (chatId) {
        dirty.delete(chatId);
        const page = await acpDesktop
          .getChatPage({
            projectId: projectId as SageProjectId,
            projectGeneration: String(epoch) as ProjectGeneration,
            chatId: chatId as SageChatId,
            cursor: null,
          })
          .catch(() => null);
        if (page && current() && page.chatId === chatId)
          mirrorAgentRailProjection(page);
      }
      pending = false;
      schedule();
    };
    const schedule = () => {
      if (!current() || pending || timer || !dirty.size) return;
      timer = setTimeout(() => {
        timer = undefined;
        void refresh();
      }, 50);
    };
    const enqueue = (chatId: string) => {
      if (
        !current() ||
        !useWorkbenchStore
          .getState()
          .tabs.some((tab) => tab.kind === "chat" && tab.id === chatId)
      )
        return;
      if (dirty.size < 64) dirty.add(chatId);
      schedule();
    };
    void acpDesktop
      .subscribeProjectionChanges((event) => {
        if (event.projectId === projectId) enqueue(event.chatId);
      })
      .then((dispose) => {
        if (!current()) {
          dispose();
          return;
        }
        unlisten = dispose;
        for (const tab of useWorkbenchStore.getState().tabs) {
          if (tab.kind === "chat") enqueue(tab.id);
        }
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      if (timer) clearTimeout(timer);
      dirty.clear();
      unlisten?.();
    };
  }, [projectId, generation, epoch, transitioning]);
}

import { useEffect, useLayoutEffect, useRef } from "react";
import { useWorkbenchStore } from "../app/store";
import type {
  ProjectGeneration,
  SageChatId,
  SageProjectId,
} from "../domain/acp";
import { isTauriRuntime } from "../ports/desktopRuntime";
import { acpDesktop } from "../services/acpDesktop";
import { mirrorAgentRailStatus } from "./useAgentRailProjection";

const MAX_DIRTY_CHATS = 64;
const STATUS_COALESCE_MS = 50;
export function useAgentRailStatus(visible: boolean) {
  const visibility = useRef(visible);
  const wake = useRef<(() => void) | null>(null);
  useLayoutEffect(() => {
    visibility.current = visible;
    if (visible) wake.current?.();
  }, [visible]);
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
    let rescanIndex: number | null = null;
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
      if (!current() || !visibility.current || pending) return;
      if (!dirty.size && rescanIndex !== null) {
        const tabs = useWorkbenchStore.getState().tabs;
        while (rescanIndex < tabs.length && dirty.size < MAX_DIRTY_CHATS) {
          const tab = tabs[rescanIndex++];
          if (tab.kind === "chat") dirty.add(tab.id);
        }
        if (rescanIndex >= tabs.length) rescanIndex = null;
      }
      pending = true;
      const chatId = dirty.values().next().value;
      if (chatId) {
        dirty.delete(chatId);
        const page = await acpDesktop
          .getChatStatus({
            projectId: projectId as SageProjectId,
            projectGeneration: String(epoch) as ProjectGeneration,
            chatId: chatId as SageChatId,
          })
          .catch(() => null);
        if (page && current() && page.chatId === chatId)
          mirrorAgentRailStatus(page);
      }
      pending = false;
      schedule();
    };
    const schedule = () => {
      if (
        !current() ||
        !visibility.current ||
        pending ||
        timer ||
        (!dirty.size && rescanIndex === null)
      )
        return;
      timer = setTimeout(() => {
        timer = undefined;
        void refresh();
      }, STATUS_COALESCE_MS);
    };
    const enqueue = (chatId: string) => {
      if (
        !current() ||
        !useWorkbenchStore
          .getState()
          .tabs.some((tab) => tab.kind === "chat" && tab.id === chatId)
      )
        return;
      if (dirty.size < MAX_DIRTY_CHATS) dirty.add(chatId);
      else if (!dirty.has(chatId)) rescanIndex ??= 0;
      schedule();
    };
    wake.current = schedule;
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
      wake.current = null;
      if (timer) clearTimeout(timer);
      dirty.clear();
      unlisten?.();
    };
  }, [projectId, generation, epoch, transitioning]);
}

export default function AgentRailStatusObserver({
  visible,
}: {
  visible: boolean;
}) {
  useAgentRailStatus(visible);
  return null;
}

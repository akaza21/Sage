import { createContext, useContext, useEffect } from "react";
import type { AcpChatPage } from "../domain/acp";
import { presentedAttentionPosition } from "../services/chatAttention";
import { chatAttentionStore } from "./chatAttentionStore";
import { useWorkbenchStore } from "./store";

const DetailVisible = createContext(true);
export const AttentionDetailVisible = DetailVisible.Provider;
export const useAttentionDetailVisible = () => useContext(DetailVisible);

export function usePresentedAttention(page: AcpChatPage, focused: boolean) {
  const visible = useAttentionDetailVisible();
  useEffect(() => {
    const state = useWorkbenchStore.getState();
    if (state.projectTransitioning || state.project?.id !== page.projectId)
      return;
    const tab = state.tabs.find(
      (tab) => tab.kind === "chat" && tab.id === page.chatId,
    );
    if (!tab || tab.title === page.title) return;
    const latest = page.turns.at(-1);
    useWorkbenchStore.setState({
      tabs: state.tabs.map((entry) =>
        entry === tab
          ? {
              ...entry,
              title: page.title,
              updatedAt: latest
                ? Math.max(entry.updatedAt ?? 0, latest.updatedAt)
                : entry.updatedAt,
            }
          : entry,
      ),
    });
  }, [page]);
  useEffect(() => {
    if (!visible || !focused) return;
    const position = presentedAttentionPosition(page);
    if (position === undefined) return;
    let pending = false;
    const present = () => {
      if (pending) return;
      pending = true;
      void chatAttentionStore
        .present({
          projectId: page.projectId,
          chatId: page.chatId,
          observedPosition: position,
          presented: true,
          foreground:
            document.visibilityState === "visible" && document.hasFocus(),
          agent: visible,
          settings: false,
        })
        .finally(() => {
          pending = false;
        });
    };
    let frame = requestAnimationFrame(present);
    const unsubscribe = chatAttentionStore.subscribe(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(present);
    });
    window.addEventListener("focus", present);
    document.addEventListener("visibilitychange", present);
    return () => {
      cancelAnimationFrame(frame);
      unsubscribe();
      window.removeEventListener("focus", present);
      document.removeEventListener("visibilitychange", present);
    };
  }, [page, focused, visible]);
}

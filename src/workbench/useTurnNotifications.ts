import { useEffect } from "react";
import { openNotifiedChat } from "../app/openChat";
import { useWorkbenchStore } from "../app/store";
import { useAgentSettingsState } from "../features/settings/agentSettingsState";
import { nativeNotificationPort } from "../ports/nativeNotificationPort";
import { isTauriRuntime } from "../ports/nativeInvoke";
import {
  listenNotificationNavigation,
  observeNotificationVisibility,
} from "./notificationLifecycle";

export function useNotificationVisibility(
  chatIds: readonly string[],
  visible: boolean,
) {
  const ids = JSON.stringify(chatIds);
  useEffect(() => {
    if (!isTauriRuntime()) return;
    return observeNotificationVisibility(
      nativeNotificationPort,
      JSON.parse(ids) as string[],
      visible,
    );
  }, [ids, visible]);
}

export function useNotificationNavigation(enterAgent: () => void) {
  useEffect(() => {
    if (!isTauriRuntime()) return;
    let disposed = false;
    const failed = () => {
      if (!disposed)
        useWorkbenchStore.setState({
          error: "The notification chat could not be opened.",
        });
    };
    // A failed open or switch keeps its own error.
    const unreported = () => {
      if (!useWorkbenchStore.getState().error) failed();
    };
    // A click during a project switch waits in the held-open slot, loaded
    // through the lazy rail entry so it adds no core chunk. A
    // chat deleted meanwhile is still handed to land, which reports it.
    const notified = {
      open: async (projectId: string, chatId: string) => {
        if (disposed) return;
        await openNotifiedChat(projectId, chatId).catch(() => {});
        const current = useWorkbenchStore.getState();
        if (current.project?.id !== projectId || current.activeTabId !== chatId)
          return unreported();
        useAgentSettingsState.getState().close();
        enterAgent();
      },
      refused: unreported,
    };
    const navigation = listenNotificationNavigation(
      nativeNotificationPort,
      async ({ projectId, chatId }) =>
        (await import("./ProjectChatNavigation")).openChatWhenSettled(
          projectId,
          chatId,
          notified,
        ),
      failed,
    );
    return () => {
      disposed = true;
      navigation.dispose();
    };
  }, [enterAgent]);
}

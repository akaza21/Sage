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
    const failed = () =>
      useWorkbenchStore.setState({
        error: "The notification chat could not be opened.",
      });
    const land = async (projectId: string, chatId: string) => {
      await openNotifiedChat(projectId, chatId).catch(() => {});
      const current = useWorkbenchStore.getState();
      if (current.project?.id !== projectId || current.activeTabId !== chatId)
        return failed();
      useAgentSettingsState.getState().close();
      enterAgent();
    };
    // A click during a project switch waits in the rail's held-open slot,
    // which loads with the lazy rail chunk.
    const navigation = listenNotificationNavigation(
      nativeNotificationPort,
      async ({ projectId, chatId }) =>
        (await import("./ProjectChatNavigation")).openChatWhenSettled(
          projectId,
          chatId,
          land,
        ),
      failed,
    );
    return () => navigation.dispose();
  }, [enterAgent]);
}

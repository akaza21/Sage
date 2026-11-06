import { useEffect } from "react";
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
    const navigation = listenNotificationNavigation(
      nativeNotificationPort,
      () => !useWorkbenchStore.getState().projectTransitioning,
      async ({ projectId, chatId }) => {
        await useWorkbenchStore.getState().openRailChat(projectId, chatId);
        const current = useWorkbenchStore.getState();
        if (
          current.project?.id !== projectId ||
          current.activeTabId !== chatId
        ) {
          throw new Error("Notification chat is no longer available");
        }
        useAgentSettingsState.getState().close();
        enterAgent();
      },
      () =>
        useWorkbenchStore.setState({
          error: "The notification chat could not be opened.",
        }),
    );
    const unsubscribe = useWorkbenchStore.subscribe((state, before) => {
      if (before.projectTransitioning && !state.projectTransitioning)
        navigation.wake();
    });
    return () => {
      unsubscribe();
      navigation.dispose();
    };
  }, [enterAgent]);
}

import { useEffect } from "react";
import { useWorkbenchStore } from "../app/store";
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
    // A click during a project switch waits in the rail's held-open slot,
    // loaded with the lazy rail chunk.
    const navigation = listenNotificationNavigation(
      nativeNotificationPort,
      async ({ projectId, chatId }) =>
        (await import("./ProjectChatNavigation")).openNotificationChat(
          projectId,
          chatId,
          enterAgent,
        ),
      () =>
        useWorkbenchStore.setState({
          error: "The notification chat could not be opened.",
        }),
    );
    return () => navigation.dispose();
  }, [enterAgent]);
}

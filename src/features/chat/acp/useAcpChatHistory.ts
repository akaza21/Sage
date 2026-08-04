import { useCallback } from "react";

import { useAcpProjectionStore } from "../../../app/acpProjectionStore";
import { useWorkbenchStore } from "../../../app/store";
import type { ProjectGeneration } from "../../../domain/acp";
import { acpDesktop } from "../../../services/acpDesktop";

export function useAcpChatHistory() {
  const page = useAcpProjectionStore((state) => state.page);
  const scope = useAcpProjectionStore((state) => state.scope);
  const activeLoad = useAcpProjectionStore((state) => state.activeLoad);
  const error = useAcpProjectionStore((state) => state.error);

  const loadEarlier = useCallback(() => {
    const projection = useAcpProjectionStore.getState();
    const workbench = useWorkbenchStore.getState();
    const cursor = projection.page?.nextCursor;
    if (
      !projection.scope ||
      !cursor ||
      projection.activeLoad ||
      workbench.projectTransitioning ||
      workbench.project?.id !== projection.scope.projectId ||
      String(workbench.projectGeneration) !==
        projection.scope.projectGeneration ||
      workbench.activeTabId !== projection.scope.chatId
    ) {
      return;
    }
    const intent = projection.beginPageLoad(cursor);
    void acpDesktop
      .getChatPage({
        projectId: intent.projectId,
        projectGeneration: String(
          workbench.nativeTransitionEpoch,
        ) as ProjectGeneration,
        chatId: intent.chatId,
        cursor,
      })
      .then((historyPage) => {
        useAcpProjectionStore.getState().completePageLoad(intent, historyPage);
      })
      .catch((loadError: unknown) => {
        useAcpProjectionStore
          .getState()
          .failPageLoad(
            intent,
            loadError instanceof Error ? loadError.message : String(loadError),
          );
      });
  }, []);

  return {
    hasEarlier: Boolean(page?.nextCursor && scope),
    loadingEarlier: activeLoad?.cursor !== null && activeLoad !== null,
    historyError: error,
    loadEarlier,
  };
}

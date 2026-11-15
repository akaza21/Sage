import { useCallback } from "react";
import { useStore } from "zustand";

import { acpProjectionStoreFor } from "../../../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../../app/store";
import type { ProjectGeneration } from "../../../domain/acp";
import { errorMessage } from "../../../domain/errors";
import { getGoalChatPage } from "../../../services/acpGoalDesktop";

export function useAcpChatHistory(boundChatId?: string | null) {
  const store = acpProjectionStoreFor(boundChatId);
  const page = useStore(store, (state) => state.page);
  const scope = useStore(store, (state) => state.scope);
  const activeLoad = useStore(store, (state) => state.activeLoad);
  const historyLoad = useStore(store, (state) => state.historyLoad);
  const historyError = useStore(store, (state) => state.historyError);
  const firstPageError = useStore(store, (state) =>
    state.page ? null : state.error,
  );
  // A chat is resolved in a settled project, so its first page is on the
  // way even before the projection's effect has begun loading it.
  const pageExpected = useWorkbenchStore((state) => {
    const chatId =
      boundChatId ??
      (state.tabs.find((tab) => tab.id === state.activeTabId)?.kind === "chat"
        ? state.activeTabId
        : null);
    return !!chatId && !!state.project && !state.projectTransitioning;
  });

  const loadEarlier = useCallback(() => {
    const projection = acpProjectionStoreFor(boundChatId).getState();
    const workbench = useWorkbenchStore.getState();
    const cursor = projection.page?.nextCursor;
    if (
      !projection.scope ||
      !cursor ||
      projection.historyLoad ||
      workbench.projectTransitioning ||
      workbench.project?.id !== projection.scope.projectId ||
      String(workbench.projectGeneration) !==
        projection.scope.projectGeneration ||
      (boundChatId
        ? projection.scope.chatId !== boundChatId
        : workbench.activeTabId !== projection.scope.chatId)
    ) {
      return;
    }
    const intent = projection.beginPageLoad(cursor);
    void getGoalChatPage({
      projectId: intent.projectId,
      projectGeneration: String(
        workbench.nativeTransitionEpoch,
      ) as ProjectGeneration,
      chatId: intent.chatId,
      cursor,
    })
      .then((historyPage) => {
        acpProjectionStoreFor(boundChatId)
          .getState()
          .completePageLoad(intent, historyPage);
      })
      .catch((loadError: unknown) => {
        acpProjectionStoreFor(boundChatId)
          .getState()
          .failPageLoad(intent, errorMessage(loadError));
      });
  }, [boundChatId]);

  return {
    historyLoading: activeLoad !== null || historyLoad !== null,
    pageExpected,
    hasEarlier: Boolean(page?.nextCursor && scope),
    loadingEarlier: historyLoad !== null,
    historyError,
    firstPageError,
    onLoadEarlier: loadEarlier,
  };
}

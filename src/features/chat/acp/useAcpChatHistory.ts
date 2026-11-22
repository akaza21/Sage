import { useCallback } from "react";
import { useStore } from "zustand";

import { acpProjectionStoreFor } from "../../../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../../app/store";
import { errorMessage } from "../../../domain/errors";
import { getGoalChatPage } from "../../../services/acpGoalDesktop";
import { currentContext } from "./useAcpChatActions";

export function useAcpChatHistory(boundChatId?: string | null) {
  const store = acpProjectionStoreFor(boundChatId);
  const page = useStore(store, (state) => state.page);
  const scope = useStore(store, (state) => state.scope);
  const activeLoad = useStore(store, (state) => state.activeLoad);
  const historyLoad = useStore(store, (state) => state.historyLoad);
  const historyError = useStore(store, (state) => state.historyError);
  const onReloadHistory = useStore(store, (state) => state.requestReload);
  const resolvedChatId = useWorkbenchStore(
    (state) =>
      boundChatId ??
      (state.tabs.find((tab) => tab.id === state.activeTabId)?.kind === "chat"
        ? state.activeTabId
        : null),
  );
  const firstPageError = useStore(store, (state) =>
    state.page ? null : state.error,
  );
  // A chat is resolved in a settled project, so its first page is on the
  // way even before the projection's effect has begun loading it.
  const projectReady = useWorkbenchStore(
    (state) => !!state.project && !state.projectTransitioning,
  );

  const loadEarlier = useCallback(() => {
    const projection = acpProjectionStoreFor(boundChatId).getState();
    const context = currentContext(boundChatId);
    const cursor = context?.page.nextCursor;
    if (!context || !cursor || projection.historyLoad) return;
    const intent = projection.beginPageLoad(cursor);
    void getGoalChatPage({
      projectId: intent.projectId,
      projectGeneration: context.commandProjectGeneration,
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
    pageExpected: !!resolvedChatId && projectReady,
    hasEarlier: Boolean(page?.nextCursor && scope),
    loadingEarlier: historyLoad !== null,
    historyError,
    firstPageError,
    onLoadEarlier: loadEarlier,
    onReloadHistory,
    historyScope: JSON.stringify([
      scope?.projectId,
      resolvedChatId,
      scope?.projectGeneration,
    ]),
  };
}

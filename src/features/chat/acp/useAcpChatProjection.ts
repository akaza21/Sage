import { useEffect } from "react";
import { useStore } from "zustand";

import {
  acpProjectionStoreFor,
  releaseAcpProjectionStore,
} from "../../../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../../app/store";
import type {
  ProjectGeneration,
  SageChatId,
  SageProjectId,
} from "../../../domain/acp";
import { acpDesktop } from "../../../services/acpDesktop";

export function useAcpRetentionIncomplete(boundChatId?: string | null) {
  return useStore(
    acpProjectionStoreFor(boundChatId),
    (state) => state.retentionIncomplete,
  );
}

export function useAcpChatProjection(
  enabled = true,
  boundChatId?: string | null,
) {
  const projectId = useWorkbenchStore((state) => state.project?.id ?? null);
  const projectGeneration = useWorkbenchStore(
    (state) => state.projectGeneration,
  );
  const nativeTransitionEpoch = useWorkbenchStore(
    (state) => state.nativeTransitionEpoch,
  );
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const activeTabKind = useWorkbenchStore(
    (state) =>
      state.tabs.find((tab) => tab.id === state.activeTabId)?.kind ?? null,
  );
  const resolvedChatId =
    boundChatId ?? (activeTabKind === "chat" ? activeTabId : null);
  const store = acpProjectionStoreFor(boundChatId);
  const page = useStore(store, (state) => state.page);
  const projectionScope = useStore(store, (state) => state.scope);
  const activateScope = useStore(store, (state) => state.activateScope);
  const beginPageLoad = useStore(store, (state) => state.beginPageLoad);
  const completePageLoad = useStore(store, (state) => state.completePageLoad);
  const failPageLoad = useStore(store, (state) => state.failPageLoad);
  const clear = useStore(store, (state) => state.clear);
  const enqueueProjectionChange = useStore(
    store,
    (state) => state.enqueueProjectionChange,
  );
  const takeProjectionChange = useStore(
    store,
    (state) => state.takeProjectionChange,
  );

  useEffect(() => {
    if (!boundChatId) return;
    return () => releaseAcpProjectionStore(boundChatId);
  }, [boundChatId]);

  useEffect(() => {
    if (!enabled || !projectId || !resolvedChatId || projectTransitioning) {
      clear();
      return;
    }

    const scope = {
      projectId: projectId as SageProjectId,
      projectGeneration: String(projectGeneration) as ProjectGeneration,
      chatId: resolvedChatId as SageChatId,
    };
    activateScope(scope);
    let disposed = false;
    let refreshTimer: ReturnType<typeof setTimeout> | null = null;
    let unlisten: (() => void) | null = null;

    const loadLatest = () => {
      if (disposed) return;
      const intent = beginPageLoad(null);
      void acpDesktop
        .getChatPage({
          projectId: scope.projectId,
          projectGeneration: String(nativeTransitionEpoch) as ProjectGeneration,
          chatId: scope.chatId,
          cursor: null,
        })
        .then((projection) => {
          completePageLoad(intent, projection);
        })
        .catch((error: unknown) => {
          failPageLoad(
            intent,
            error instanceof Error ? error.message : String(error),
          );
        });
    };

    void acpDesktop
      .subscribeProjectionChanges((event) => {
        if (disposed) return;
        const disposition = enqueueProjectionChange(event);
        if (disposition === "ignoredStale" || refreshTimer) return;
        refreshTimer = setTimeout(() => {
          refreshTimer = null;
          takeProjectionChange();
          loadLatest();
        }, 50);
      })
      .then((disposeListener) => {
        if (disposed) {
          disposeListener();
        } else {
          unlisten = disposeListener;
          loadLatest();
        }
      })
      .catch(() => {
        // Bootstrap remains authoritative when the disposable event listener
        // is unavailable. A later scope activation retries once.
        loadLatest();
      });

    return () => {
      disposed = true;
      if (refreshTimer) clearTimeout(refreshTimer);
      unlisten?.();
    };
  }, [
    activateScope,
    resolvedChatId,
    beginPageLoad,
    clear,
    completePageLoad,
    enqueueProjectionChange,
    enabled,
    failPageLoad,
    nativeTransitionEpoch,
    projectGeneration,
    projectId,
    projectTransitioning,
    takeProjectionChange,
  ]);

  if (
    !enabled ||
    projectTransitioning ||
    !projectId ||
    !resolvedChatId ||
    projectionScope?.projectId !== projectId ||
    projectionScope.projectGeneration !== String(projectGeneration) ||
    projectionScope.chatId !== resolvedChatId
  ) {
    return null;
  }
  return page;
}

import { useEffect } from "react";

import { useAcpProjectionStore } from "../../../app/acpProjectionStore";
import { useWorkbenchStore } from "../../../app/store";
import type {
  ProjectGeneration,
  SageChatId,
  SageProjectId,
} from "../../../domain/acp";
import { acpDesktop } from "../../../services/acpDesktop";

export function useAcpRetentionIncomplete() {
  return useAcpProjectionStore((state) => state.retentionIncomplete);
}

export function useAcpChatProjection(enabled = true) {
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
  const page = useAcpProjectionStore((state) => state.page);
  const projectionScope = useAcpProjectionStore((state) => state.scope);
  const activateScope = useAcpProjectionStore((state) => state.activateScope);
  const beginPageLoad = useAcpProjectionStore((state) => state.beginPageLoad);
  const completePageLoad = useAcpProjectionStore(
    (state) => state.completePageLoad,
  );
  const failPageLoad = useAcpProjectionStore((state) => state.failPageLoad);
  const clear = useAcpProjectionStore((state) => state.clear);
  const enqueueProjectionChange = useAcpProjectionStore(
    (state) => state.enqueueProjectionChange,
  );
  const takeProjectionChange = useAcpProjectionStore(
    (state) => state.takeProjectionChange,
  );

  useEffect(() => {
    if (
      !enabled ||
      !projectId ||
      !activeTabId ||
      activeTabKind !== "chat" ||
      projectTransitioning
    ) {
      clear();
      return;
    }

    const scope = {
      projectId: projectId as SageProjectId,
      projectGeneration: String(projectGeneration) as ProjectGeneration,
      chatId: activeTabId as SageChatId,
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
    activeTabId,
    activeTabKind,
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
    !activeTabId ||
    activeTabKind !== "chat" ||
    projectionScope?.projectId !== projectId ||
    projectionScope.projectGeneration !== String(projectGeneration) ||
    projectionScope.chatId !== activeTabId
  ) {
    return null;
  }
  return page;
}

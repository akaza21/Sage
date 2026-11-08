import { useContext, useEffect } from "react";
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
import { errorMessage } from "../../../domain/errors";
import { acpDesktop } from "../../../services/acpDesktop";
import {
  getGoalChatPage,
  subscribeGoalChanges,
} from "../../../services/acpGoalDesktop";

import { AcpProjectionOwnerContext } from "./AcpProjectionOwnerContext";
import { coalescedRefresh } from "./coalescedRefresh";

/**
 * A refresh may overtake one load that hangs (the store keeps only the newest
 * answer), never more. With the live snapshot's one read and the history's
 * one earlier page, that is the four page reads the native side admits per
 * Chat at once. Status and file-snapshot reads share that limit, so they can
 * meet a capacity error while all four are out.
 */
const PAGE_LOADS_IN_FLIGHT = 2;

export function useAcpRetentionIncomplete(boundChatId?: string | null) {
  return useStore(
    acpProjectionStoreFor(boundChatId),
    (state) => state.retentionIncomplete,
  );
}

export function useAcpChatLoadFailure(boundChatId?: string | null) {
  const store = acpProjectionStoreFor(boundChatId);
  const error = useStore(store, (state) => state.error);
  const retry = useStore(store, (state) => state.requestReload);
  return { error, retry };
}

export function useAcpChatProjection(
  enabled = true,
  boundChatId?: string | null,
) {
  const owners = useContext(AcpProjectionOwnerContext);
  const externallyOwned = !!boundChatId && owners?.has(boundChatId) === true;
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
  const loadNonce = useStore(store, (state) => state.loadNonce);
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
    if (externallyOwned) return;
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
    let unlisten: (() => void) | null = null;

    const refresh = coalescedRefresh(() => {
      const intent = beginPageLoad(null);
      return getGoalChatPage({
        projectId: scope.projectId,
        projectGeneration: String(nativeTransitionEpoch) as ProjectGeneration,
        chatId: scope.chatId,
        cursor: null,
      })
        .then((projection) => {
          completePageLoad(intent, projection);
        })
        .catch((error: unknown) => {
          failPageLoad(intent, errorMessage(error));
        });
    }, PAGE_LOADS_IN_FLIGHT);

    const scheduleRefresh = (disposition: string) => {
      if (disposed || disposition === "ignoredStale") return;
      refresh.schedule(() => {
        takeProjectionChange();
        store.getState().takeSessionConfigChange();
      });
    };
    // Install both wake channels before the authoritative bootstrap snapshot.
    void Promise.allSettled([
      acpDesktop.subscribeProjectionChanges((event) =>
        scheduleRefresh(enqueueProjectionChange(event)),
      ),
      acpDesktop.subscribeSessionConfigChanges((event) =>
        scheduleRefresh(
          store.getState().enqueueSessionConfigChange(scope, event),
        ),
      ),
      subscribeGoalChanges((event) => {
        const page = store.getState().page;
        if (
          event.sageProjectId === scope.projectId &&
          event.sageChatId === scope.chatId &&
          (!page ||
            (page.currentSessionId === event.sageAgentSessionId &&
              page.sessions.find(
                (session) => session.id === event.sageAgentSessionId,
              )?.runtimeEpoch === event.runtimeEpoch))
        )
          scheduleRefresh("goalChanged");
      }),
    ])
      .then((listeners) => {
        const disposeListener = () =>
          listeners.forEach((listener) => {
            if (listener.status === "fulfilled") listener.value();
          });
        if (disposed) {
          disposeListener();
        } else {
          unlisten = disposeListener;
          refresh.now();
        }
      })
      .catch(() => {
        // Bootstrap remains authoritative when the disposable event listener
        // is unavailable. A later scope activation retries once.
        refresh.now();
      });

    return () => {
      disposed = true;
      refresh.dispose();
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
    externallyOwned,
    failPageLoad,
    loadNonce,
    nativeTransitionEpoch,
    projectGeneration,
    projectId,
    projectTransitioning,
    takeProjectionChange,
    store,
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

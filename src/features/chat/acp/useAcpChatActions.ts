import { useMemo } from "react";
import { useStore } from "zustand";

import { acpProjectionStoreFor } from "../../../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../../app/store";
import type {
  CuratedBindingId,
  CuratedModelId,
  ProjectGeneration,
  SageTurnId,
} from "../../../domain/acp";
import type { ChatOwnershipIntent } from "../../../domain/chatOwnership";
import {
  type AcpActionContext,
  type AcpActionState,
  cancellableTurnId,
  createAcpActionController,
} from "./acpActionController";

function currentContext(boundChatId?: string | null): AcpActionContext | null {
  const projection = acpProjectionStoreFor(boundChatId).getState();
  const workbench = useWorkbenchStore.getState();
  if (
    !projection.scope ||
    !projection.page ||
    workbench.projectTransitioning ||
    workbench.project?.id !== projection.scope.projectId ||
    String(workbench.projectGeneration) !==
      projection.scope.projectGeneration ||
    (boundChatId
      ? projection.scope.chatId !== boundChatId
      : workbench.activeTabId !== projection.scope.chatId)
  ) {
    return null;
  }
  const ownership = workbench.captureChatOwnership(projection.scope.chatId);
  if (!ownership) return null;
  return {
    scope: projection.scope,
    ownershipEpoch: ownership.ownershipEpoch,
    commandProjectGeneration: String(
      workbench.nativeTransitionEpoch,
    ) as ProjectGeneration,
    page: projection.page,
  };
}

function ownershipIntent(context: AcpActionContext): ChatOwnershipIntent {
  return {
    projectId: context.scope.projectId,
    ownershipEpoch: context.ownershipEpoch,
    chatId: context.scope.chatId,
    projectGeneration: Number(context.scope.projectGeneration),
    transitionEpoch: Number(context.commandProjectGeneration),
  };
}

export function useAcpChatActions(boundChatId?: string | null) {
  const store = acpProjectionStoreFor(boundChatId);
  const projectionRevision = useStore(
    store,
    (projection) => projection.page?.projectionRevision ?? null,
  );
  const page = useStore(store, (projection) => projection.page);
  const sending = useWorkbenchStore((state) =>
    boundChatId
      ? (state.chatUiById[boundChatId]?.sending ?? false)
      : state.sending,
  );
  const chatError = useWorkbenchStore((state) =>
    boundChatId
      ? (state.chatUiById[boundChatId]?.error ?? null)
      : state.chatError,
  );
  const clearActiveChatError = useWorkbenchStore(
    (state) => state.clearActiveChatError,
  );
  const captureChatOwnership = useWorkbenchStore(
    (state) => state.captureChatOwnership,
  );
  const setChatActionState = useWorkbenchStore(
    (state) => state.setChatActionState,
  );

  const controller = useMemo(
    () =>
      createAcpActionController({
        getContext: () => currentContext(boundChatId),
        applyAuthoritativePage: (scope, authoritativePage) => {
          return acpProjectionStoreFor(boundChatId)
            .getState()
            .applyAuthoritativePage(scope, authoritativePage);
        },
        onStateChange: (state: AcpActionState) => {
          if (
            !state.scope ||
            state.ownershipEpoch === null ||
            state.transitionEpoch === null
          )
            return;
          const workbench = useWorkbenchStore.getState();
          workbench.setChatActionState(
            {
              projectId: state.scope.projectId,
              ownershipEpoch: state.ownershipEpoch,
              chatId: state.scope.chatId,
              projectGeneration: Number(state.scope.projectGeneration),
              transitionEpoch: state.transitionEpoch,
            },
            { sending: state.busy, error: state.error?.message ?? null },
          );
        },
        clearComposer: (context, prompt) => {
          const workbench = useWorkbenchStore.getState();
          workbench.clearChatComposer(ownershipIntent(context), prompt);
        },
        restoreComposer: (context, prompt) => {
          const workbench = useWorkbenchStore.getState();
          workbench.restoreChatComposer(ownershipIntent(context), prompt);
        },
      }),
    [boundChatId],
  );

  // Reading the revision makes authoritative mutation responses re-evaluate the
  // exact cancellable Turn even though the controller itself is deliberately
  // detached from disposable React rendering state.
  void projectionRevision;
  return {
    busy: sending,
    error: chatError,
    cancellableTurnId: page ? cancellableTurnId(page) : null,
    submit: (
      prompt: string,
      bindingId: CuratedBindingId,
      modelId: CuratedModelId | null,
    ) => controller.submit(prompt, bindingId, modelId),
    cancel: (turnId: SageTurnId) => controller.cancel(turnId),
    close: () => controller.close(),
    clearError: () => {
      if (boundChatId) {
        const ownership = captureChatOwnership(boundChatId);
        if (!ownership) return;
        const entry = useWorkbenchStore.getState().chatUiById[boundChatId];
        setChatActionState(ownership, {
          sending: entry?.sending ?? false,
          error: null,
        });
        return;
      }
      clearActiveChatError();
    },
  } satisfies {
    busy: boolean;
    error: string | null;
    cancellableTurnId: SageTurnId | null;
    submit: typeof controller.submit;
    cancel: typeof controller.cancel;
    close: typeof controller.close;
    clearError: () => void;
  };
}

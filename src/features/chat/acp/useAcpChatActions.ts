import { useMemo } from "react";
import { useStore } from "zustand";

import { acpProjectionStoreFor } from "../../../app/acpProjectionRegistry";
import { useSessionAccessModeStore } from "../../../app/sessionAccessModeStore";
import { useWorkbenchStore } from "../../../app/store";
import type { AttachedContext } from "../../../domain/attachedContext";
import type {
  AcpCommandFailure,
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
  const errorKind = useWorkbenchStore((state) =>
    boundChatId
      ? (state.chatUiById[boundChatId]?.errorKind ?? null)
      : state.activeTabId
        ? (state.chatUiById[state.activeTabId]?.errorKind ?? null)
        : null,
  );
  const modeError = useWorkbenchStore((state) => {
    const chatId = boundChatId ?? state.activeTabId;
    return chatId ? (state.chatUiById[chatId]?.modeError ?? null) : null;
  });
  const modeErrorOwner = useWorkbenchStore((state) => {
    const chatId = boundChatId ?? state.activeTabId;
    return chatId ? (state.chatUiById[chatId]?.modeErrorOwner ?? null) : null;
  });
  const modeChanging = useWorkbenchStore((state) => {
    const chatId = boundChatId ?? state.activeTabId;
    return chatId ? (state.chatUiById[chatId]?.modeChanging ?? false) : false;
  });
  const modeOwnerMatches =
    modeErrorOwner?.sessionId === page?.sessionConfig?.sageAgentSessionId &&
    modeErrorOwner?.runtimeEpoch === page?.sessionConfig?.runtimeEpoch;
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
        onUnscopedRefusal: (message) => {
          const workbench = useWorkbenchStore.getState();
          const ownership = workbench.captureChatOwnership(
            boundChatId ?? undefined,
          );
          if (!ownership) return;
          const entry = workbench.chatUiById[ownership.chatId];
          workbench.setChatActionState(ownership, {
            sending: entry?.sending ?? false,
            error: message,
            errorKind: "invalidRequest",
          });
        },
        settleAccessMode: () => useSessionAccessModeStore.getState().settle(),
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
            state.origin === "mode"
              ? {
                  sending:
                    workbench.chatUiById[state.scope.chatId]?.sending ?? false,
                  error:
                    workbench.chatUiById[state.scope.chatId]?.error ?? null,
                  modeError: state.error?.message ?? null,
                  modeErrorOwner: state.configOwner,
                  modeChanging: state.busy,
                }
              : {
                  sending: state.busy,
                  error: state.error?.message ?? null,
                  errorKind: state.error?.failure.kind ?? null,
                  modeError:
                    workbench.chatUiById[state.scope.chatId]?.modeError ?? null,
                  modeErrorOwner:
                    workbench.chatUiById[state.scope.chatId]?.modeErrorOwner ??
                    null,
                  modeChanging:
                    workbench.chatUiById[state.scope.chatId]?.modeChanging ??
                    false,
                },
          );
        },
        clearComposer: (context, prompt, attachedContext) => {
          const workbench = useWorkbenchStore.getState();
          workbench.clearChatComposer(
            ownershipIntent(context),
            prompt,
            attachedContext,
          );
        },
        restoreComposer: (context, prompt, attachedContext) => {
          const workbench = useWorkbenchStore.getState();
          workbench.restoreChatComposer(
            ownershipIntent(context),
            prompt,
            attachedContext,
          );
        },
      }),
    [boundChatId],
  );

  void projectionRevision;
  return {
    busy: sending,
    error: chatError,
    errorKind,
    modeError: modeOwnerMatches ? modeError : null,
    modeChanging: modeOwnerMatches && modeChanging,
    cancellableTurnId: page ? cancellableTurnId(page) : null,
    submit: (
      prompt: string,
      attachedContext: readonly AttachedContext[],
      bindingId: CuratedBindingId,
      modelId: CuratedModelId | null,
    ) => controller.submit(prompt, attachedContext, bindingId, modelId),
    steer: (
      prompt: string,
      attachedContext: readonly AttachedContext[],
      queuedMessageId?: Parameters<typeof controller.steer>[2],
    ) => controller.steer(prompt, attachedContext, queuedMessageId),
    cancel: (turnId: SageTurnId) => controller.cancel(turnId),
    close: () => controller.close(),
    end: () => controller.end(),
    setSessionMode: controller.setSessionMode,
    setSessionConfigOption: controller.setSessionConfigOption,
    clearError: () => {
      if (boundChatId) {
        const ownership = captureChatOwnership(boundChatId);
        if (!ownership) return;
        const entry = useWorkbenchStore.getState().chatUiById[boundChatId];
        setChatActionState(ownership, {
          sending: entry?.sending ?? false,
          error: null,
          errorKind: null,
          modeError: entry?.modeError ?? null,
          modeErrorOwner: entry?.modeErrorOwner ?? null,
        });
        return;
      }
      clearActiveChatError();
    },
  } satisfies {
    busy: boolean;
    error: string | null;
    errorKind: AcpCommandFailure["kind"] | null;
    modeError: string | null;
    modeChanging: boolean;
    cancellableTurnId: SageTurnId | null;
    submit: typeof controller.submit;
    steer: typeof controller.steer;
    cancel: typeof controller.cancel;
    close: typeof controller.close;
    end: typeof controller.end;
    setSessionMode: typeof controller.setSessionMode;
    setSessionConfigOption: typeof controller.setSessionConfigOption;
    clearError: () => void;
  };
}

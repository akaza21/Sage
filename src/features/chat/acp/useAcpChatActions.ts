import { useMemo } from "react";
import { useStore } from "zustand";

import { acpProjectionStoreFor } from "../../../app/acpProjectionRegistry";
import { useSessionAccessModeStore } from "../../../app/sessionAccessModeStore";
import { useWorkbenchStore, type WorkbenchState } from "../../../app/store";
import { harnessSignInFailures } from "../../../services/acpHarnessDesktop";
import type {
  AcpCommandFailure,
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

export function currentContext(
  boundChatId?: string | null,
): AcpActionContext | null {
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
  const page = useStore(store, (projection) => projection.page);
  const chatUi = (state: WorkbenchState) =>
    state.chatUiById[boundChatId ?? state.activeTabId ?? ""];
  const sending = useWorkbenchStore((state) =>
    boundChatId ? (chatUi(state)?.sending ?? false) : state.sending,
  );
  const chatError = useWorkbenchStore((state) =>
    boundChatId ? (chatUi(state)?.error ?? null) : state.chatError,
  );
  const errorKind = useWorkbenchStore(
    (state) => chatUi(state)?.errorKind ?? null,
  );
  const modeError = useWorkbenchStore(
    (state) => chatUi(state)?.modeError ?? null,
  );
  const modeErrorOwner = useWorkbenchStore(
    (state) => chatUi(state)?.modeErrorOwner ?? null,
  );
  const modeChanging = useWorkbenchStore(
    (state) => chatUi(state)?.modeChanging ?? false,
  );
  const modeOwnerMatches =
    modeErrorOwner?.sessionId === page?.sessionConfig?.sageAgentSessionId &&
    modeErrorOwner?.runtimeEpoch === page?.sessionConfig?.runtimeEpoch;

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
          const chat = workbench.chatUiById[state.scope.chatId];
          const accepted = workbench.setChatActionState(
            {
              projectId: state.scope.projectId,
              ownershipEpoch: state.ownershipEpoch,
              chatId: state.scope.chatId,
              projectGeneration: Number(state.scope.projectGeneration),
              transitionEpoch: state.transitionEpoch,
            },
            state.origin === "mode"
              ? {
                  sending: chat?.sending ?? false,
                  error: chat?.error ?? null,
                  modeError: state.error?.message ?? null,
                  modeErrorOwner: state.configOwner,
                  modeChanging: state.busy,
                }
              : {
                  sending: state.busy,
                  error: state.error?.message ?? null,
                  errorKind: state.error?.failure.kind ?? null,
                  errorBindingId: state.error ? state.bindingId : null,
                },
          );
          if (accepted) harnessSignInFailures.command?.(state);
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

  return {
    busy: sending,
    error: chatError,
    errorKind,
    modeError: modeOwnerMatches ? modeError : null,
    modeChanging: modeOwnerMatches && modeChanging,
    cancellableTurnId: page ? cancellableTurnId(page) : null,
    submit: controller.submit,
    runSessionAction: controller.runSessionAction,
    steer: controller.steer,
    cancel: controller.cancel,
    close: controller.close,
    end: controller.end,
    setSessionMode: controller.setSessionMode,
    setSessionConfigOption: controller.setSessionConfigOption,
  } satisfies {
    busy: boolean;
    error: string | null;
    errorKind: AcpCommandFailure["kind"] | null;
    modeError: string | null;
    modeChanging: boolean;
    cancellableTurnId: SageTurnId | null;
    submit: typeof controller.submit;
    runSessionAction: typeof controller.runSessionAction;
    steer: typeof controller.steer;
    cancel: typeof controller.cancel;
    close: typeof controller.close;
    end: typeof controller.end;
    setSessionMode: typeof controller.setSessionMode;
    setSessionConfigOption: typeof controller.setSessionConfigOption;
  };
}

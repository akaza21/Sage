import { useMemo, useState } from "react";

import {
  type AcpProjectionScope,
  useAcpProjectionStore,
} from "../../../app/acpProjectionStore";
import { useWorkbenchStore } from "../../../app/store";
import type {
  CuratedBindingId,
  CuratedModelId,
  ProjectGeneration,
  SageTurnId,
} from "../../../domain/acp";
import type { AcpDesktopError } from "../../../services/acpDesktop";
import {
  type AcpActionContext,
  type AcpActionState,
  cancellableTurnId,
  createAcpActionController,
} from "./acpActionController";

const initialState: AcpActionState = {
  scope: null,
  busy: false,
  cancellingTurnId: null,
  error: null,
};

function sameScope(
  left: AcpProjectionScope | null,
  right: AcpProjectionScope,
): boolean {
  return (
    left?.projectId === right.projectId &&
    left.projectGeneration === right.projectGeneration &&
    left.chatId === right.chatId
  );
}

function currentContext(): AcpActionContext | null {
  const projection = useAcpProjectionStore.getState();
  const workbench = useWorkbenchStore.getState();
  if (
    !projection.scope ||
    !projection.page ||
    workbench.projectTransitioning ||
    workbench.project?.id !== projection.scope.projectId ||
    String(workbench.projectGeneration) !==
      projection.scope.projectGeneration ||
    workbench.activeTabId !== projection.scope.chatId
  ) {
    return null;
  }
  return {
    scope: projection.scope,
    commandProjectGeneration: String(
      workbench.nativeTransitionEpoch,
    ) as ProjectGeneration,
    page: projection.page,
  };
}

export function useAcpChatActions() {
  const [state, setState] = useState<AcpActionState>(initialState);
  const projectionRevision = useAcpProjectionStore(
    (projection) => projection.page?.projectionRevision ?? null,
  );
  const page = useAcpProjectionStore((projection) => projection.page);
  const activeScope = useAcpProjectionStore((projection) => projection.scope);

  const controller = useMemo(
    () =>
      createAcpActionController({
        getContext: currentContext,
        applyAuthoritativePage: (scope, authoritativePage) => {
          return useAcpProjectionStore
            .getState()
            .applyAuthoritativePage(scope, authoritativePage);
        },
        onStateChange: setState,
        clearComposer: (scope, prompt) => {
          const projection = useAcpProjectionStore.getState();
          const workbench = useWorkbenchStore.getState();
          if (
            sameScope(projection.scope, scope) &&
            workbench.composerText.trim() === prompt
          ) {
            useWorkbenchStore.setState({ composerText: "" });
          }
        },
        restoreComposer: (scope, prompt) => {
          const projection = useAcpProjectionStore.getState();
          const workbench = useWorkbenchStore.getState();
          if (sameScope(projection.scope, scope) && !workbench.composerText) {
            useWorkbenchStore.setState({ composerText: prompt });
          }
        },
      }),
    [],
  );

  // Reading the revision makes authoritative mutation responses re-evaluate the
  // exact cancellable Turn even though the controller itself is deliberately
  // detached from disposable React rendering state.
  void projectionRevision;
  const stateIsCurrent =
    activeScope !== null && sameScope(state.scope, activeScope);

  return {
    busy: stateIsCurrent && state.busy,
    error: stateIsCurrent ? state.error : null,
    cancellableTurnId:
      page && (!stateIsCurrent || !state.cancellingTurnId)
        ? cancellableTurnId(page)
        : null,
    submit: (
      prompt: string,
      bindingId: CuratedBindingId,
      modelId: CuratedModelId | null,
    ) => controller.submit(prompt, bindingId, modelId),
    cancel: (turnId: SageTurnId) => controller.cancel(turnId),
    close: () => controller.close(),
    clearError: () => setState((current) => ({ ...current, error: null })),
  } satisfies {
    busy: boolean;
    error: AcpDesktopError | null;
    cancellableTurnId: SageTurnId | null;
    submit: typeof controller.submit;
    cancel: typeof controller.cancel;
    close: typeof controller.close;
    clearError: () => void;
  };
}

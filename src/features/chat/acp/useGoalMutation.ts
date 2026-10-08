import { useMemo } from "react";
import { acpProjectionStoreFor } from "../../../app/acpProjectionRegistry";
import type {
  AcpChatPage,
  AcpMutationResult,
  AcpSessionMutationIntent,
} from "../../../domain/acp";
import { AcpDesktopError, acpDesktop } from "../../../services/acpDesktop";
import type { GoalMutationOutcome } from "../ChatGoalControl";
import { createClientIntentFactory } from "./clientIntent";
import { currentContext } from "./useAcpChatActions";
import type { AcpActionContext } from "./acpActionController";

const GOAL_PROTOCOL = "codex-app-server-observed-0.146.0";

export function canMutateGoal(page: AcpChatPage | null | undefined): boolean {
  const session = page?.sessions.find(
    (item) => item.id === page.currentSessionId,
  );
  return (
    page?.goal?.kind === "goal" &&
    !!session?.runtimeEpoch &&
    session.capabilities.protocolVersion === GOAL_PROTOCOL
  );
}

type Action = "pause" | "clear";

export function createGoalMutationController(deps: {
  getContext: () => AcpActionContext | null;
  pause: (request: AcpSessionMutationIntent) => Promise<AcpMutationResult>;
  clear: (request: AcpSessionMutationIntent) => Promise<AcpMutationResult>;
  apply: (context: AcpActionContext, result: AcpMutationResult) => boolean;
  nextIntentId: ReturnType<typeof createClientIntentFactory>;
}) {
  let pending = false;
  let unknown = false;
  return async (action: Action): Promise<GoalMutationOutcome> => {
    if (pending || unknown) {
      return {
        kind: "refused",
        message: "Goal action is already pending verification.",
      };
    }
    const captured = deps.getContext();
    if (!captured || !canMutateGoal(captured.page)) {
      return {
        kind: "refused",
        message: "Native Goal mutation is unavailable for this session.",
      };
    }
    const session = captured.page.sessions.find(
      (item) => item.id === captured.page.currentSessionId,
    )!;
    if (
      action === "pause" &&
      captured.page.goal?.kind === "goal" &&
      captured.page.goal.snapshot.status !== "active"
    ) {
      return { kind: "refused", message: "This Goal is not active." };
    }
    pending = true;
    try {
      const current = deps.getContext();
      if (
        !current ||
        current.scope.projectId !== captured.scope.projectId ||
        current.scope.chatId !== captured.scope.chatId ||
        current.scope.projectGeneration !== captured.scope.projectGeneration ||
        current.ownershipEpoch !== captured.ownershipEpoch ||
        current.commandProjectGeneration !==
          captured.commandProjectGeneration ||
        current.page.currentSessionId !== session.id ||
        current.page.sessions.find((item) => item.id === session.id)
          ?.runtimeEpoch !== session.runtimeEpoch ||
        !canMutateGoal(current.page)
      ) {
        return {
          kind: "refused",
          message: "Goal session changed before the action could start.",
        };
      }
      const request: AcpSessionMutationIntent = {
        ...current.scope,
        projectGeneration: current.commandProjectGeneration,
        expectedProjectionRevision: current.page.projectionRevision,
        clientIntentId: deps.nextIntentId(),
        sessionId: session.id,
        runtimeEpoch: session.runtimeEpoch!,
      };
      const result = await (action === "pause"
        ? deps.pause(request)
        : deps.clear(request));
      if (!deps.apply(captured, result)) {
        unknown = true;
        return {
          kind: "outcomeUnknown",
          message: "Goal outcome unknown. Reload this Chat to verify.",
        };
      }
      return { kind: "applied" };
    } catch (error) {
      const kind =
        error instanceof AcpDesktopError ? error.failure.kind : "internal";
      if (kind === "recoveryRequired" || kind === "internal") {
        unknown = true;
        return {
          kind: "outcomeUnknown",
          message: "Goal outcome unknown. Reload this Chat to verify.",
        };
      }
      return {
        kind: "refused",
        message:
          error instanceof AcpDesktopError
            ? error.message
            : "Native Goal refused the action.",
      };
    } finally {
      pending = false;
    }
  };
}

export function useGoalMutation(boundChatId?: string | null) {
  return useMemo(
    () =>
      createGoalMutationController({
        getContext: () => currentContext(boundChatId),
        pause: acpDesktop.pauseExistingGoal,
        clear: acpDesktop.clearExistingGoal,
        apply: (captured, result) => {
          const current = currentContext(boundChatId);
          if (
            !current ||
            current.ownershipEpoch !== captured.ownershipEpoch ||
            current.commandProjectGeneration !==
              captured.commandProjectGeneration ||
            current.scope.projectId !== captured.scope.projectId ||
            current.scope.chatId !== captured.scope.chatId ||
            current.scope.projectGeneration !==
              captured.scope.projectGeneration ||
            current.page.currentSessionId !== captured.page.currentSessionId ||
            current.page.sessions.find(
              (session) => session.id === current.page.currentSessionId,
            )?.runtimeEpoch !==
              captured.page.sessions.find(
                (session) => session.id === captured.page.currentSessionId,
              )?.runtimeEpoch
          )
            return false;
          return (
            acpProjectionStoreFor(boundChatId)
              .getState()
              .applyAuthoritativePage(captured.scope, result.projection) ===
            "accepted"
          );
        },
        nextIntentId: createClientIntentFactory(),
      }),
    [boundChatId],
  );
}

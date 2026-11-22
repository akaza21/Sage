import { goalMutations } from "../../../services/acpGoalMutations";
import type { GoalMutationAction } from "../../../domain/acpGoal";
import { useMemo } from "react";
import { acpProjectionStoreFor } from "../../../app/acpProjectionRegistry";
import type {
  AcpMutationResult,
  AcpSessionMutationIntent,
} from "../../../domain/acp";
import { AcpDesktopError } from "../../../services/acpDesktop";
import type { GoalMutationOutcome } from "../ChatGoalControl";
import { createClientIntentFactory } from "./clientIntent";
import { currentContext } from "./useAcpChatActions";
import type { AcpActionContext } from "./acpActionController";
import { sameScope } from "./acpActionProjection";
import { canMutateGoal } from "./goalMutability";

type Action = GoalMutationAction;

export function createGoalMutationController(deps: {
  getContext: () => AcpActionContext | null;
  set?: (
    request: AcpSessionMutationIntent & {
      objective: string;
      tokenBudget?: number;
    },
  ) => Promise<AcpMutationResult>;
  resume?: (request: AcpSessionMutationIntent) => Promise<AcpMutationResult>;
  tokenBudget?: (
    request: AcpSessionMutationIntent & { tokenBudget: number },
  ) => Promise<AcpMutationResult>;
  pause: (request: AcpSessionMutationIntent) => Promise<AcpMutationResult>;
  clear: (request: AcpSessionMutationIntent) => Promise<AcpMutationResult>;
  apply: (context: AcpActionContext, result: AcpMutationResult) => boolean;
  nextIntentId: ReturnType<typeof createClientIntentFactory>;
}) {
  let pending = false;
  let unknown = false;
  const outcomeUnknown = (): GoalMutationOutcome => {
    unknown = true;
    return {
      kind: "outcomeUnknown",
      message: "Goal outcome unknown. Reload this chat to verify.",
    };
  };
  return async (
    action: Action,
    objective?: string,
    tokenBudget?: number,
  ): Promise<GoalMutationOutcome> => {
    if (pending || unknown) {
      return {
        kind: "refused",
        message: "Goal action is already pending verification.",
      };
    }
    const captured = deps.getContext();
    if (!captured || !canMutateGoal(captured.page, action)) {
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
    if (
      action === "resume" &&
      (captured.page.goal?.kind !== "goal" ||
        !["paused", "usageLimited", "budgetLimited"].includes(
          captured.page.goal.snapshot.status,
        ))
    ) {
      return { kind: "refused", message: "This Goal cannot be resumed." };
    }
    pending = true;
    try {
      const current = deps.getContext();
      if (
        !sameScope(current, captured) ||
        current.page.currentSessionId !== session.id ||
        current.page.sessions.find((item) => item.id === session.id)
          ?.runtimeEpoch !== session.runtimeEpoch ||
        !canMutateGoal(current.page, action)
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
      if (
        action === "set" &&
        (!deps.set ||
          !objective?.trim() ||
          [...objective].length > 4000 ||
          objective.includes("\0"))
      ) {
        return {
          kind: "refused",
          message: "Enter a goal of 1 to 4,000 characters.",
        };
      }
      if (
        (tokenBudget !== undefined &&
          (!session.capabilities.goal?.tokenBudget ||
            !Number.isSafeInteger(tokenBudget) ||
            tokenBudget <= 0)) ||
        (action === "tokenBudget" &&
          (tokenBudget === undefined || !deps.tokenBudget)) ||
        (action === "resume" && !deps.resume)
      ) {
        return {
          kind: "refused",
          message:
            "Enter a positive integer token budget within the supported range.",
        };
      }
      const operations = {
        set: () =>
          deps.set!({
            ...request,
            objective: objective!,
            ...(tokenBudget === undefined ? {} : { tokenBudget }),
          }),
        pause: () => deps.pause(request),
        resume: () => deps.resume!(request),
        tokenBudget: () =>
          deps.tokenBudget!({ ...request, tokenBudget: tokenBudget! }),
        clear: () => deps.clear(request),
      };
      const result = await operations[action]();
      if (!deps.apply(captured, result)) {
        return outcomeUnknown();
      }
      return { kind: "applied" };
    } catch (error) {
      if (
        !(error instanceof AcpDesktopError) ||
        error.failure.kind === "recoveryRequired" ||
        error.failure.kind === "internal"
      ) {
        return outcomeUnknown();
      }
      return {
        kind: "refused",
        message: error.message,
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
        set: goalMutations.setGoal,
        pause: goalMutations.pauseExistingGoal,
        resume: goalMutations.resumeExistingGoal,
        tokenBudget: goalMutations.setGoalBudget,
        clear: goalMutations.clearExistingGoal,
        apply: (captured, result) => {
          const current = currentContext(boundChatId);
          if (
            !sameScope(current, captured) ||
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

import { validateAttachedContexts } from "../../../domain/attachedContext";
import type { AttachedContext } from "../../../domain/attachedContext";
import type {
  AcpChatPage,
  AcpConfigValue,
  AcpMutationResult,
  CancelAcpTurnRequest,
  ClientIntentId,
  CloseAcpSessionRequest,
  EndAcpSessionRequest,
  SageTurnId,
  SteerAcpTurnRequest,
} from "../../../domain/acp";
import { AcpDesktopError } from "../../../services/acpDesktop";
import type { acpDesktop } from "../../../services/acpDesktop";
import type {
  AcpActionContext,
  AcpActionOrigin,
  AcpActionOutcome,
  AcpActionState,
} from "./acpActionController";
import { cancellableTurnId } from "./acpTurnState";
import { currentNonterminalTurn } from "./currentNonterminalTurn";

interface SessionActionDependencies {
  getContext: () => AcpActionContext | null;
  enqueue: (
    captured: AcpActionContext,
    origin: AcpActionOrigin,
    action: (
      updateState: (
        next: Partial<
          Pick<AcpActionState, "busy" | "cancellingTurnId" | "error">
        >,
      ) => void,
    ) => Promise<AcpActionOutcome>,
  ) => Promise<AcpActionOutcome>;
  apply: (
    captured: AcpActionContext,
    result: AcpMutationResult,
  ) => AcpChatPage | null;
  sameScope: (
    context: AcpActionContext | null,
    expected: AcpActionContext,
  ) => boolean;
  nextIntentId: () => ClientIntentId;
  cancelTurn: (request: CancelAcpTurnRequest) => Promise<AcpMutationResult>;
  steerTurn: (request: SteerAcpTurnRequest) => Promise<AcpMutationResult>;
  closeSession: (request: CloseAcpSessionRequest) => Promise<AcpMutationResult>;
  endSession: (request: EndAcpSessionRequest) => Promise<AcpMutationResult>;
  setSessionMode?: typeof acpDesktop.setSessionMode;
  setSessionConfigOption?: typeof acpDesktop.setSessionConfigOption;
}

export function currentSession(page: AcpChatPage) {
  if (!page.currentSessionId) return null;
  return (
    page.sessions.find((session) => session.id === page.currentSessionId) ??
    null
  );
}

function typedError(error: unknown): AcpDesktopError {
  if (error instanceof AcpDesktopError) return error;
  return new AcpDesktopError({
    kind: "internal",
    message: "The ACP action failed safely.",
  });
}

export function createAcpSessionActions(
  dependencies: SessionActionDependencies,
) {
  const steer = (
    prompt: string,
    attachedContext: readonly AttachedContext[],
    queuedMessageId?: ClientIntentId,
  ): Promise<AcpActionOutcome> => {
    const captured = dependencies.getContext();
    if (!captured) return Promise.resolve("rejected");
    let boundedContext: readonly AttachedContext[];
    try {
      boundedContext = validateAttachedContexts(attachedContext);
    } catch {
      return Promise.resolve("rejected");
    }
    const normalizedPrompt = prompt.trim();
    const active = currentNonterminalTurn(captured.page);
    const session = currentSession(captured.page);
    if (
      !normalizedPrompt ||
      !active ||
      !session?.runtimeEpoch ||
      !session.capabilities?.activeTurnSteer
    )
      return Promise.resolve("rejected");
    return dependencies.enqueue(captured, "chat", async (updateState) => {
      const current = dependencies.getContext();
      if (!dependencies.sameScope(current, captured) || !current)
        return "ignoredStale";
      const currentTurn = currentNonterminalTurn(current.page);
      const currentSessionValue = currentSession(current.page);
      if (
        currentTurn?.id !== active.id ||
        !currentSessionValue?.runtimeEpoch ||
        !currentSessionValue.capabilities?.activeTurnSteer
      )
        return "rejected";
      const request: SteerAcpTurnRequest = {
        ...captured.scope,
        projectGeneration: captured.commandProjectGeneration,
        expectedProjectionRevision: current.page.projectionRevision,
        clientIntentId: queuedMessageId ?? dependencies.nextIntentId(),
        sessionId: currentSessionValue.id,
        runtimeEpoch: currentSessionValue.runtimeEpoch,
        turnId: currentTurn.id,
        expectedTurnRevision: currentTurn.revision,
        prompt: normalizedPrompt,
        ...(boundedContext.length > 0
          ? { attachedContext: boundedContext }
          : {}),
      };
      try {
        const sent = await dependencies.steerTurn(request);
        dependencies.apply(captured, sent);
        return dependencies.sameScope(dependencies.getContext(), captured)
          ? "applied"
          : "ignoredStale";
      } catch (value) {
        if (
          value instanceof AcpDesktopError &&
          value.failure.kind === "conflict" &&
          value.failure.resource === "steerOutcomeUnknown"
        ) {
          return "outcomeUnknown";
        }
        updateState({ error: typedError(value) });
        return "rejected";
      }
    });
  };
  const cancel = (turnId: SageTurnId): Promise<AcpActionOutcome> => {
    const captured = dependencies.getContext();
    if (!captured || cancellableTurnId(captured.page) !== turnId) {
      return Promise.resolve("rejected");
    }
    return dependencies.enqueue(captured, "chat", async (updateState) => {
      const context = dependencies.getContext();
      if (!dependencies.sameScope(context, captured) || !context) {
        return "ignoredStale";
      }
      const session = currentSession(context.page);
      const turn = context.page.turns.find(
        (candidate) => candidate.id === turnId,
      );
      if (
        !session?.runtimeEpoch ||
        !turn ||
        cancellableTurnId(context.page) !== turnId
      ) {
        return "rejected";
      }
      updateState({ cancellingTurnId: turnId });
      try {
        const request: CancelAcpTurnRequest = {
          ...captured.scope,
          projectGeneration: captured.commandProjectGeneration,
          expectedProjectionRevision: context.page.projectionRevision,
          clientIntentId: dependencies.nextIntentId(),
          sessionId: session.id,
          runtimeEpoch: session.runtimeEpoch,
          turnId,
          expectedTurnState: turn.state,
          expectedTurnRevision: turn.revision,
        };
        const result = await dependencies.cancelTurn(request);
        return dependencies.apply(captured, result)
          ? "applied"
          : "ignoredStale";
      } catch (value) {
        updateState({ error: typedError(value) });
        return "rejected";
      } finally {
        updateState({ cancellingTurnId: null });
      }
    });
  };

  const close = (): Promise<AcpActionOutcome> => {
    const captured = dependencies.getContext();
    const session = captured ? currentSession(captured.page) : null;
    if (
      !captured ||
      !session?.runtimeEpoch ||
      !session.capabilities.closeSession
    ) {
      return Promise.resolve("rejected");
    }
    return dependencies.enqueue(captured, "chat", async (updateState) => {
      const context = dependencies.getContext();
      if (!dependencies.sameScope(context, captured) || !context) {
        return "ignoredStale";
      }
      const current = currentSession(context.page);
      if (!current?.runtimeEpoch || !current.capabilities.closeSession) {
        return "rejected";
      }
      try {
        const result = await dependencies.closeSession({
          ...captured.scope,
          projectGeneration: captured.commandProjectGeneration,
          expectedProjectionRevision: context.page.projectionRevision,
          clientIntentId: dependencies.nextIntentId(),
          sessionId: current.id,
          runtimeEpoch: current.runtimeEpoch,
        });
        return dependencies.apply(captured, result)
          ? "applied"
          : "ignoredStale";
      } catch (value) {
        updateState({ error: typedError(value) });
        return "rejected";
      }
    });
  };

  const end = (): Promise<AcpActionOutcome> => {
    const captured = dependencies.getContext();
    const session = captured ? currentSession(captured.page) : null;
    if (
      !captured ||
      !session?.runtimeEpoch ||
      session.capabilities.closeSession
    ) {
      return Promise.resolve("rejected");
    }
    return dependencies.enqueue(captured, "chat", async (updateState) => {
      const context = dependencies.getContext();
      if (!dependencies.sameScope(context, captured) || !context) {
        return "ignoredStale";
      }
      const current = currentSession(context.page);
      if (!current?.runtimeEpoch || current.capabilities.closeSession) {
        return "rejected";
      }
      try {
        const result = await dependencies.endSession({
          ...captured.scope,
          projectGeneration: captured.commandProjectGeneration,
          expectedProjectionRevision: context.page.projectionRevision,
          clientIntentId: dependencies.nextIntentId(),
          sessionId: current.id,
          runtimeEpoch: current.runtimeEpoch,
        });
        return dependencies.apply(captured, result)
          ? "applied"
          : "ignoredStale";
      } catch (value) {
        updateState({ error: typedError(value) });
        return "rejected";
      }
    });
  };

  const configure = (
    mutation:
      | { modeId: string }
      | {
          configId: string;
          value: AcpConfigValue;
        },
  ): Promise<AcpActionOutcome> => {
    const captured = dependencies.getContext();
    const config = captured?.page.sessionConfig;
    const modeId = "modeId" in mutation ? mutation.modeId : null;
    if (!captured || !config) return Promise.resolve("rejected");
    if (
      modeId !== null &&
      (config.removed ||
        !config.modes ||
        !config.modes.availableModes.some((mode) => mode.id === modeId))
    ) {
      return Promise.resolve("rejected");
    }
    return dependencies.enqueue(captured, "mode", async (updateState) => {
      const context = dependencies.getContext();
      if (!context || !dependencies.sameScope(context, captured))
        return "ignoredStale";
      const session = currentSession(context.page);
      const current = context.page.sessionConfig;
      if (
        !session?.runtimeEpoch ||
        !current ||
        (modeId !== null &&
          (current.removed ||
            !current.modes ||
            !current.modes.availableModes.some(
              (mode) => mode.id === modeId,
            ))) ||
        current.sageAgentSessionId !== config.sageAgentSessionId ||
        current.runtimeEpoch !== config.runtimeEpoch ||
        current.configRevision !== config.configRevision ||
        (modeId !== null && current.modes?.currentModeId === modeId)
      )
        return "ignoredStale";
      const request = {
        ...captured.scope,
        projectGeneration: captured.commandProjectGeneration,
        expectedProjectionRevision: context.page.projectionRevision,
        clientIntentId: dependencies.nextIntentId(),
        sessionId: session.id,
        runtimeEpoch: session.runtimeEpoch,
        expectedConfigRevision: current.configRevision,
      };
      try {
        const result =
          "modeId" in mutation
            ? await dependencies.setSessionMode?.({ ...request, ...mutation })
            : await dependencies.setSessionConfigOption?.({
                ...request,
                ...mutation,
              });
        if (!result) return "rejected";
        return dependencies.apply(captured, result)
          ? "applied"
          : "ignoredStale";
      } catch (error) {
        updateState({ error: typedError(error) });
        return "rejected";
      }
    });
  };
  return {
    steer,
    cancel,
    close,
    end,
    setSessionMode: (modeId: string) => configure({ modeId }),
    setSessionConfigOption: (configId: string, value: AcpConfigValue) =>
      configure({ configId, value }),
  };
}

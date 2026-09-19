import type {
  AcpChatPage,
  AcpConfigValue,
  AcpMutationResult,
  CancelAcpTurnRequest,
  ClientIntentId,
  CloseAcpSessionRequest,
  SageTurnId,
} from "../../../domain/acp";
import { AcpDesktopError } from "../../../services/acpDesktop";
import type { acpDesktop } from "../../../services/acpDesktop";
import type {
  AcpActionContext,
  AcpActionOutcome,
  AcpActionState,
} from "./acpActionController";
import { cancellableTurnId } from "./acpTurnState";

interface SessionActionDependencies {
  getContext: () => AcpActionContext | null;
  enqueue: (
    captured: AcpActionContext,
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
  closeSession: (request: CloseAcpSessionRequest) => Promise<AcpMutationResult>;
  setSessionMode?: typeof acpDesktop.setSessionMode;
  setSessionConfigOption?: typeof acpDesktop.setSessionConfigOption;
}

function currentSession(page: AcpChatPage) {
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
  const cancel = (turnId: SageTurnId): Promise<AcpActionOutcome> => {
    const captured = dependencies.getContext();
    if (!captured || cancellableTurnId(captured.page) !== turnId) {
      return Promise.resolve("rejected");
    }
    return dependencies.enqueue(captured, async (updateState) => {
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
    return dependencies.enqueue(captured, async (updateState) => {
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
    if (!captured || !config) return Promise.resolve("rejected");
    return dependencies.enqueue(captured, async (updateState) => {
      const context = dependencies.getContext();
      if (!context || !dependencies.sameScope(context, captured))
        return "ignoredStale";
      const session = currentSession(context.page);
      const current = context.page.sessionConfig;
      if (
        !session?.runtimeEpoch ||
        !current ||
        current.sageAgentSessionId !== config.sageAgentSessionId ||
        current.runtimeEpoch !== config.runtimeEpoch ||
        current.configRevision !== config.configRevision
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
    cancel,
    close,
    setSessionMode: (modeId: string) => configure({ modeId }),
    setSessionConfigOption: (configId: string, value: AcpConfigValue) =>
      configure({ configId, value }),
  };
}

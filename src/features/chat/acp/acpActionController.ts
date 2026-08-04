import type {
  AcpChatPage,
  AcpMutationResult,
  AgentSessionProjection,
  CancelAcpTurnRequest,
  ClientIntentId,
  CuratedBindingId,
  CuratedModelId,
  ProjectGeneration,
  SageTurnId,
  SendAcpTurnRequest,
  TurnProjection,
} from "../../../domain/acp";
import type {
  AcpProjectionScope,
  ProjectionCommitDisposition,
} from "../../../app/acpProjectionStore";
import { AcpDesktopError, acpDesktop } from "../../../services/acpDesktop";
import { createClientIntentFactory } from "./clientIntent";

const MAX_QUEUED_ACTIONS = 32;
const MAX_QUEUED_CHAT_SCOPES = 8;

const cancellableStates = new Set<TurnProjection["state"]>([
  "queued",
  "sending",
  "running",
  "waitingModel",
  "waitingTool",
  "waitingApproval",
  "waitingUser",
]);

export interface AcpActionContext {
  scope: AcpProjectionScope;
  commandProjectGeneration: ProjectGeneration;
  page: AcpChatPage;
}

export interface AcpActionState {
  scope: AcpProjectionScope | null;
  busy: boolean;
  cancellingTurnId: SageTurnId | null;
  error: AcpDesktopError | null;
}

export interface AcpActionCommands {
  startSession: typeof acpDesktop.startSession;
  activateSession: typeof acpDesktop.activateSession;
  sendTurn: typeof acpDesktop.sendTurn;
  cancelTurn: typeof acpDesktop.cancelTurn;
  closeSession: typeof acpDesktop.closeSession;
}

interface AcpActionDependencies {
  commands?: AcpActionCommands;
  getContext: () => AcpActionContext | null;
  applyAuthoritativePage: (
    scope: AcpProjectionScope,
    page: AcpChatPage,
  ) => ProjectionCommitDisposition;
  onStateChange?: (state: AcpActionState) => void;
  clearComposer?: (scope: AcpProjectionScope, prompt: string) => void;
  restoreComposer?: (scope: AcpProjectionScope, prompt: string) => void;
  createIntentId?: () => ClientIntentId;
}

export type AcpActionOutcome = "applied" | "ignoredStale" | "rejected";

function typedError(error: unknown): AcpDesktopError {
  if (error instanceof AcpDesktopError) return error;
  return new AcpDesktopError({
    kind: "internal",
    message: "The ACP action failed safely.",
  });
}

function localFailure(message: string): AcpDesktopError {
  return new AcpDesktopError({ kind: "invalidRequest", message });
}

function sameScope(
  context: AcpActionContext | null,
  expected: AcpActionContext,
): boolean {
  return (
    context?.scope.projectId === expected.scope.projectId &&
    context.scope.projectGeneration === expected.scope.projectGeneration &&
    context.scope.chatId === expected.scope.chatId &&
    context.commandProjectGeneration === expected.commandProjectGeneration
  );
}

function scopeKey(context: AcpActionContext): string {
  return JSON.stringify([
    context.scope.projectId,
    context.scope.projectGeneration,
    context.scope.chatId,
    context.commandProjectGeneration,
  ]);
}

function currentSession(page: AcpChatPage): AgentSessionProjection | null {
  if (!page.currentSessionId) return null;
  return (
    page.sessions.find((session) => session.id === page.currentSessionId) ??
    null
  );
}

function sessionById(
  page: AcpChatPage,
  sessionId: AgentSessionProjection["id"],
): AgentSessionProjection {
  const session = page.sessions.find((candidate) => candidate.id === sessionId);
  if (!session) throw localFailure("The authoritative session is unavailable.");
  return session;
}

function requireSelectedSession(
  session: AgentSessionProjection,
  bindingId: CuratedBindingId,
  modelId: CuratedModelId | null,
): AgentSessionProjection {
  if (session.bindingId !== bindingId || session.modelId !== modelId) {
    throw localFailure(
      "The authoritative session does not match the selected harness and model.",
    );
  }
  return session;
}

function definitePromptNonDispatch(error: AcpDesktopError): boolean {
  return !["recoveryRequired", "internal"].includes(error.failure.kind);
}

export function cancellableTurnId(page: AcpChatPage): SageTurnId | null {
  const session = currentSession(page);
  if (!session?.runtimeEpoch || !session.capabilities.cancel) return null;
  const active = page.turns
    .filter(
      (turn) =>
        turn.sessionId === session.id &&
        turn.runtimeEpoch === session.runtimeEpoch &&
        cancellableStates.has(turn.state),
    )
    .sort((left, right) => right.ordinal - left.ordinal);
  return active.length === 1 ? active[0].id : null;
}

export function createAcpActionController(dependencies: AcpActionDependencies) {
  const commands = dependencies.commands ?? acpDesktop;
  const nextIntentId =
    dependencies.createIntentId ?? createClientIntentFactory();
  const queues = new Map<string, Promise<unknown>>();
  let queuedActions = 0;
  let state: AcpActionState = {
    scope: null,
    busy: false,
    cancellingTurnId: null,
    error: null,
  };

  const updateState = (next: Partial<AcpActionState>) => {
    state = { ...state, ...next };
    dependencies.onStateChange?.(state);
  };

  const apply = (
    captured: AcpActionContext,
    result: AcpMutationResult,
  ): AcpChatPage | null => {
    if (!sameScope(dependencies.getContext(), captured)) return null;
    const disposition = dependencies.applyAuthoritativePage(
      captured.scope,
      result.projection,
    );
    return disposition === "accepted" ? result.projection : null;
  };

  const enqueue = (
    captured: AcpActionContext,
    action: () => Promise<AcpActionOutcome>,
  ): Promise<AcpActionOutcome> => {
    const key = scopeKey(captured);
    if (
      queuedActions >= MAX_QUEUED_ACTIONS ||
      (!queues.has(key) && queues.size >= MAX_QUEUED_CHAT_SCOPES)
    ) {
      const error = new AcpDesktopError({
        kind: "capacity",
        resource: "queue",
        limit: MAX_QUEUED_ACTIONS,
        message: "Too many ACP actions are queued for this chat.",
      });
      updateState({ scope: captured.scope, error });
      return Promise.resolve("rejected");
    }
    queuedActions += 1;
    updateState({ scope: captured.scope, busy: true, error: null });
    const previous = queues.get(key) ?? Promise.resolve();
    const result = previous.then(async () => {
      if (!sameScope(dependencies.getContext(), captured)) {
        return "ignoredStale" as const;
      }
      return action();
    });
    const tail = result
      .catch(() => undefined)
      .finally(() => {
        queuedActions -= 1;
        if (queues.get(key) === tail) queues.delete(key);
        if (
          !queues.has(key) &&
          sameScope(dependencies.getContext(), captured)
        ) {
          updateState({ busy: false });
        }
      });
    queues.set(key, tail);
    return result;
  };

  const submit = (
    prompt: string,
    bindingId: CuratedBindingId,
    modelId: CuratedModelId | null,
  ): Promise<AcpActionOutcome> => {
    const captured = dependencies.getContext();
    const normalizedPrompt = prompt.trim();
    if (!captured || !normalizedPrompt) return Promise.resolve("rejected");
    return enqueue(captured, async () => {
      dependencies.clearComposer?.(captured.scope, normalizedPrompt);
      let page = dependencies.getContext()?.page ?? captured.page;
      let promptCallStarted = false;
      try {
        let session = currentSession(page);
        if (
          session &&
          (session.bindingId !== bindingId || session.modelId !== modelId)
        ) {
          session = null;
        }
        if (!session) {
          const started = await commands.startSession({
            ...captured.scope,
            projectGeneration: captured.commandProjectGeneration,
            expectedProjectionRevision: page.projectionRevision,
            clientIntentId: nextIntentId(),
            bindingId,
            modelId,
          });
          const applied = apply(captured, started);
          if (!applied) return "ignoredStale";
          page = applied;
          session = currentSession(page);
          if (!session)
            throw localFailure("Session start returned no session.");
          session = requireSelectedSession(session, bindingId, modelId);
        }

        if (session.residency === "cold") {
          if (!session.nextRuntimeEpoch) {
            throw localFailure(
              "The session has no authoritative activation epoch.",
            );
          }
          const activated = await commands.activateSession({
            ...captured.scope,
            projectGeneration: captured.commandProjectGeneration,
            expectedProjectionRevision: page.projectionRevision,
            clientIntentId: nextIntentId(),
            sessionId: session.id,
            runtimeEpoch: session.nextRuntimeEpoch,
          });
          const applied = apply(captured, activated);
          if (!applied) return "ignoredStale";
          page = applied;
          session = requireSelectedSession(
            sessionById(page, session.id),
            bindingId,
            modelId,
          );
        }

        session = requireSelectedSession(session, bindingId, modelId);

        if (!session.runtimeEpoch || !session.capabilities.prompt) {
          throw new AcpDesktopError({
            kind: "unavailable",
            reason: "capabilityUnavailable",
            message: "This session cannot accept a prompt.",
          });
        }
        const request: SendAcpTurnRequest = {
          ...captured.scope,
          projectGeneration: captured.commandProjectGeneration,
          target: "session",
          expectedProjectionRevision: page.projectionRevision,
          clientIntentId: nextIntentId(),
          sessionId: session.id,
          runtimeEpoch: session.runtimeEpoch,
          prompt: normalizedPrompt,
        };
        promptCallStarted = true;
        const sent = await commands.sendTurn(request);
        return apply(captured, sent) ? "applied" : "ignoredStale";
      } catch (value) {
        const error = typedError(value);
        if (sameScope(dependencies.getContext(), captured)) {
          updateState({ error });
          if (!promptCallStarted || definitePromptNonDispatch(error)) {
            dependencies.restoreComposer?.(captured.scope, normalizedPrompt);
          }
        }
        return "rejected";
      }
    });
  };

  const cancel = (turnId: SageTurnId): Promise<AcpActionOutcome> => {
    const captured = dependencies.getContext();
    if (!captured || cancellableTurnId(captured.page) !== turnId) {
      return Promise.resolve("rejected");
    }
    return enqueue(captured, async () => {
      const context = dependencies.getContext();
      if (!sameScope(context, captured) || !context) return "ignoredStale";
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
          clientIntentId: nextIntentId(),
          sessionId: session.id,
          runtimeEpoch: session.runtimeEpoch,
          turnId,
          expectedTurnState: turn.state,
          expectedTurnRevision: turn.revision,
        };
        const result = await commands.cancelTurn(request);
        return apply(captured, result) ? "applied" : "ignoredStale";
      } catch (value) {
        if (sameScope(dependencies.getContext(), captured)) {
          updateState({ error: typedError(value) });
        }
        return "rejected";
      } finally {
        if (sameScope(dependencies.getContext(), captured)) {
          updateState({ cancellingTurnId: null });
        }
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
    return enqueue(captured, async () => {
      const context = dependencies.getContext();
      if (!sameScope(context, captured) || !context) return "ignoredStale";
      const current = currentSession(context.page);
      if (!current?.runtimeEpoch || !current.capabilities.closeSession) {
        return "rejected";
      }
      try {
        const result = await commands.closeSession({
          ...captured.scope,
          projectGeneration: captured.commandProjectGeneration,
          expectedProjectionRevision: context.page.projectionRevision,
          clientIntentId: nextIntentId(),
          sessionId: current.id,
          runtimeEpoch: current.runtimeEpoch,
        });
        return apply(captured, result) ? "applied" : "ignoredStale";
      } catch (value) {
        if (sameScope(dependencies.getContext(), captured)) {
          updateState({ error: typedError(value) });
        }
        return "rejected";
      }
    });
  };

  return {
    submit,
    cancel,
    close,
    getState: () => state,
  };
}

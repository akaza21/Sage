import { validateAttachedContexts } from "../../../domain/attachedContext";
import type { AttachedContext } from "../../../domain/attachedContext";
import type {
  AcpChatPage,
  AgentSessionProjection,
  ClientIntentId,
  CuratedBindingId,
  CuratedModelId,
  ProjectGeneration,
  SageTurnId,
  SendAcpTurnRequest,
} from "../../../domain/acp";
import type {
  AcpProjectionScope,
  ProjectionCommitDisposition,
} from "../../../app/acpProjectionStore";
import { chatAccessDraftPort } from "../../../app/chatAccessDraftPort";
import { AcpDesktopError, acpDesktop } from "../../../services/acpDesktop";
import { createAcpSessionActions, typedError } from "./acpSessionActions";
import { currentSession } from "./acpTurnState";
import { createActionProjectionCommit, sameScope } from "./acpActionProjection";
import { createClientIntentFactory } from "./clientIntent";
import { currentNonterminalTurn } from "./currentNonterminalTurn";
import type { SlashCommandPort } from "../slashCommandTurn";
import { createSessionActionRunner } from "./sessionAction";

export { cancellableTurnId } from "./acpTurnState";

const MAX_CONCURRENT_CHAT_ACTIONS = 8;

export interface AcpActionContext {
  scope: AcpProjectionScope;
  ownershipEpoch: number;
  commandProjectGeneration: ProjectGeneration;
  page: AcpChatPage;
}

export type AcpActionOrigin = "chat" | "mode";

export interface AcpActionState {
  scope: AcpProjectionScope | null;
  ownershipEpoch: number | null;
  transitionEpoch: number | null;
  origin: AcpActionOrigin;
  configOwner: { sessionId: string; runtimeEpoch: string } | null;
  busy: boolean;
  cancellingTurnId: SageTurnId | null;
  error: AcpDesktopError | null;
}

export interface AcpActionCommands {
  startSession: typeof acpDesktop.startSession;
  activateSession: typeof acpDesktop.activateSession;
  sendTurn: typeof acpDesktop.sendTurn;
  cancelTurn: typeof acpDesktop.cancelTurn;
  steerTurn?: typeof acpDesktop.steerTurn;
  closeSession: typeof acpDesktop.closeSession;
  endSession: typeof acpDesktop.endSession;
  setSessionMode?: typeof acpDesktop.setSessionMode;
  setSessionConfigOption?: typeof acpDesktop.setSessionConfigOption;
  queueSessionConfigChoice?: typeof acpDesktop.queueSessionConfigChoice;
  runSessionAction?: typeof acpDesktop.runSessionAction;
}

interface AcpActionDependencies {
  commands?: AcpActionCommands;
  /** The typed slash-command route; loaded on demand when omitted. */
  slashCommands?: SlashCommandPort;
  getContext: () => AcpActionContext | null;
  onUnscopedRefusal?: (message: string) => void;
  settleAccessMode?: () => Promise<boolean>;
  applyAuthoritativePage: (
    scope: AcpProjectionScope,
    page: AcpChatPage,
  ) => ProjectionCommitDisposition;
  onStateChange?: (state: AcpActionState) => void;
  clearComposer?: (
    context: AcpActionContext,
    prompt: string,
    attachedContext: readonly AttachedContext[],
  ) => void;
  restoreComposer?: (
    context: AcpActionContext,
    prompt: string,
    attachedContext: readonly AttachedContext[],
  ) => void;
  createIntentId?: () => ClientIntentId;
}

export type AcpActionOutcome =
  "applied" | "ignoredStale" | "rejected" | "outcomeUnknown";
type AcpActionPresentation = Pick<
  AcpActionState,
  "busy" | "cancellingTurnId" | "error"
>;

function configOwner(page: AcpChatPage): {
  sessionId: string;
  runtimeEpoch: string;
} | null {
  const config = page.sessionConfig;
  return config
    ? {
        sessionId: config.sageAgentSessionId,
        runtimeEpoch: config.runtimeEpoch,
      }
    : null;
}

function localFailure(message: string): AcpDesktopError {
  return new AcpDesktopError({ kind: "invalidRequest", message });
}

function scopeKey(context: AcpActionContext): string {
  return JSON.stringify([
    context.scope.projectId,
    context.scope.chatId,
    context.ownershipEpoch,
  ]);
}

/** The page's head session, which activation may have replaced. */
function headSession(page: AcpChatPage): AgentSessionProjection {
  const session = currentSession(page);
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
  if (error.failure.promptDispatch) {
    return error.failure.promptDispatch === "notStarted";
  }
  return !["recoveryRequired", "internal"].includes(error.failure.kind);
}

export function createAcpActionController(dependencies: AcpActionDependencies) {
  const commands = dependencies.commands ?? acpDesktop;
  const nextIntentId =
    dependencies.createIntentId ?? createClientIntentFactory();
  const activeScopes = new Set<string>();

  const emitState = (
    captured: AcpActionContext,
    origin: AcpActionOrigin,
    presentation: AcpActionPresentation,
  ) => {
    const emitted: AcpActionState = {
      scope: captured.scope,
      ownershipEpoch: captured.ownershipEpoch,
      transitionEpoch: Number(captured.commandProjectGeneration),
      origin,
      configOwner: origin === "mode" ? configOwner(captured.page) : null,
      ...presentation,
    };
    dependencies.onStateChange?.(emitted);
  };

  const apply = createActionProjectionCommit(dependencies);

  const enqueue = (
    captured: AcpActionContext,
    origin: AcpActionOrigin,
    action: (
      updateState: (next: Partial<AcpActionPresentation>) => void,
    ) => Promise<AcpActionOutcome>,
  ): Promise<AcpActionOutcome> => {
    const key = scopeKey(captured);
    const scopeBusy = activeScopes.has(key);
    if (scopeBusy || activeScopes.size >= MAX_CONCURRENT_CHAT_ACTIONS) {
      const error = new AcpDesktopError({
        kind: "capacity",
        resource: "queue",
        limit: scopeBusy ? 1 : MAX_CONCURRENT_CHAT_ACTIONS,
        message: scopeBusy
          ? "Another action is already in progress for this Chat."
          : "Too many Chat actions are already in progress.",
      });
      emitState(captured, origin, {
        busy: false,
        cancellingTurnId: null,
        error,
      });
      return Promise.resolve("rejected");
    }
    let presentation: AcpActionPresentation = {
      busy: true,
      cancellingTurnId: null,
      error: null,
    };
    const updateState = (next: Partial<AcpActionPresentation>) => {
      presentation = { ...presentation, ...next };
      emitState(captured, origin, presentation);
    };
    activeScopes.add(key);
    emitState(captured, origin, presentation);
    const result = Promise.resolve().then(async () => {
      if (!sameScope(dependencies.getContext(), captured)) {
        return "ignoredStale" as const;
      }
      return action(updateState);
    });
    return result.finally(() => {
      activeScopes.delete(key);
      updateState({ busy: false });
    });
  };

  const submit = (
    prompt: string,
    attachedContext: readonly AttachedContext[],
    bindingId: CuratedBindingId,
    modelId: CuratedModelId | null,
  ): Promise<AcpActionOutcome> => {
    const captured = dependencies.getContext();
    const normalizedPrompt = prompt.trim();
    let boundedContext: readonly AttachedContext[];
    try {
      boundedContext = validateAttachedContexts(attachedContext);
    } catch (error) {
      if (captured) {
        emitState(captured, "chat", {
          busy: false,
          cancellingTurnId: null,
          error: localFailure(
            error instanceof Error
              ? error.message
              : "Attached context is invalid.",
          ),
        });
      }
      return Promise.resolve("rejected");
    }
    if (!captured) {
      dependencies.onUnscopedRefusal?.(
        "This Chat is not ready to send. Reload it and try again.",
      );
      return Promise.resolve("rejected");
    }
    if (!normalizedPrompt && !boundedContext.length) {
      emitState(captured, "chat", {
        busy: false,
        cancellingTurnId: null,
        error: localFailure("Enter a message before sending."),
      });
      return Promise.resolve("rejected");
    }
    if (currentNonterminalTurn(captured.page)) {
      emitState(captured, "chat", {
        busy: false,
        cancellingTurnId: null,
        error: localFailure(
          "Wait for the current Turn to finish before sending a follow-up.",
        ),
      });
      return Promise.resolve("rejected");
    }
    return enqueue(captured, "chat", async (updateState) => {
      let page = dependencies.getContext()?.page ?? captured.page;
      let promptCallStarted = false;
      try {
        if (dependencies.settleAccessMode) {
          const settled = await dependencies.settleAccessMode();
          if (!settled) {
            throw localFailure(
              "The permission mode could not be confirmed, so nothing was sent.",
            );
          }
        }
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
          // Access chosen before the session existed is given to it first.
          await chatAccessDraftPort.spend?.(captured.scope.chatId, session);
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
            headSession(page),
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
          prompt: normalizedPrompt || "Please respond to the attached context.",
          ...(boundedContext.length > 0
            ? { attachedContext: boundedContext }
            : {}),
        };
        // `/name …` may be an advertised command; its route loads on demand.
        const runCommand = normalizedPrompt.startsWith("/")
          ? await (
              await import("../slashCommandTurn")
            ).slashCommandTurn(
              request,
              nextIntentId,
              dependencies.slashCommands,
            )
          : null;
        dependencies.clearComposer?.(
          captured,
          normalizedPrompt,
          boundedContext,
        );
        promptCallStarted = true;
        const sent = await (runCommand?.() ?? commands.sendTurn(request));
        return apply(captured, sent) ? "applied" : "ignoredStale";
      } catch (value) {
        const error = typedError(value);
        updateState({ error });
        if (!promptCallStarted || definitePromptNonDispatch(error)) {
          dependencies.restoreComposer?.(
            captured,
            normalizedPrompt,
            boundedContext,
          );
        }
        return "rejected";
      }
    });
  };

  const { steer, cancel, close, end, setSessionMode, setSessionConfigOption } =
    createAcpSessionActions({
      getContext: dependencies.getContext,
      enqueue,
      apply,
      sameScope,
      nextIntentId,
      cancelTurn: commands.cancelTurn,
      steerTurn: commands.steerTurn ?? acpDesktop.steerTurn,
      closeSession: commands.closeSession,
      endSession: commands.endSession,
      setSessionMode: commands.setSessionMode,
      setSessionConfigOption: commands.setSessionConfigOption,
      queueSessionConfigChoice: commands.queueSessionConfigChoice,
    });

  const runSessionAction = createSessionActionRunner({
    getContext: dependencies.getContext,
    enqueue,
    apply,
    nextIntentId,
    run: commands.runSessionAction ?? acpDesktop.runSessionAction,
  });

  return {
    submit,
    runSessionAction,
    steer,
    cancel,
    close,
    end,
    setSessionMode,
    setSessionConfigOption,
  };
}

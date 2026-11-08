import type {
  AcpChatPage,
  AcpMutationResult,
  AcpSessionAction,
  ClientIntentId,
  RunAcpSessionActionRequest,
} from "../../../domain/acp";
import type {
  AcpActionContext,
  AcpActionOrigin,
  AcpActionOutcome,
  AcpActionState,
} from "./acpActionController";
import { typedError } from "./acpSessionActions";
import { currentSession } from "./acpTurnState";
import { currentNonterminalTurn } from "./currentNonterminalTurn";

const capabilityFor = {
  compact: "nativeSessionCompact",
  review: "nativeReview",
} as const;

/** The live, idle current session negotiated this action. */
export function sessionActionAvailable(
  page: AcpChatPage | null | undefined,
  kind: AcpSessionAction["kind"],
): boolean {
  const session = page ? currentSession(page) : null;
  return Boolean(
    session?.runtimeEpoch &&
    session.capabilities[capabilityFor[kind]] &&
    !currentNonterminalTurn(page ?? null),
  );
}

export function createSessionActionRunner(dependencies: {
  getContext: () => AcpActionContext | null;
  enqueue: (
    captured: AcpActionContext,
    origin: AcpActionOrigin,
    action: (
      updateState: (next: Partial<Pick<AcpActionState, "error">>) => void,
    ) => Promise<AcpActionOutcome>,
  ) => Promise<AcpActionOutcome>;
  apply: (
    captured: AcpActionContext,
    result: AcpMutationResult,
  ) => AcpChatPage | null;
  nextIntentId: () => ClientIntentId;
  run: (request: RunAcpSessionActionRequest) => Promise<AcpMutationResult>;
}) {
  return (action: AcpSessionAction): Promise<AcpActionOutcome> => {
    const captured = dependencies.getContext();
    if (!captured || !sessionActionAvailable(captured.page, action.kind))
      return Promise.resolve("rejected");
    return dependencies.enqueue(captured, "chat", async (updateState) => {
      const page = dependencies.getContext()?.page;
      const session = page && currentSession(page);
      if (
        !page ||
        !session?.runtimeEpoch ||
        !sessionActionAvailable(page, action.kind)
      )
        return "ignoredStale";
      try {
        const result = await dependencies.run({
          ...captured.scope,
          projectGeneration: captured.commandProjectGeneration,
          expectedProjectionRevision: page.projectionRevision,
          clientIntentId: dependencies.nextIntentId(),
          sessionId: session.id,
          runtimeEpoch: session.runtimeEpoch,
          action,
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
}

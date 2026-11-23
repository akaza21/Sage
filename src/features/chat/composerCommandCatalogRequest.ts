import type {
  AcpSessionMutationIntent,
  RuntimeEpoch,
  SageAgentSessionId,
} from "../../domain/acp";
import { createClientIntentFactory } from "./acp/clientIntent";
import { currentContext } from "./acp/useAcpChatActions";

const nextIntentId = createClientIntentFactory();

export function commandCatalogRequest(
  owner: string,
  boundChatId?: string,
): AcpSessionMutationIntent {
  const context = currentContext(boundChatId);
  if (!context) throw new Error("This chat is not ready.");
  const [sessionId, runtimeEpoch] = JSON.parse(owner) as [
    SageAgentSessionId,
    RuntimeEpoch,
  ];
  return {
    ...context.scope,
    projectGeneration: context.commandProjectGeneration,
    expectedProjectionRevision: context.page.projectionRevision,
    clientIntentId: nextIntentId(),
    sessionId,
    runtimeEpoch,
  };
}

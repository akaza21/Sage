import type {
  AcpChatPage,
  AcpItem,
  AgentSessionProjection,
  SageTurnId,
  TurnProjection,
} from "../../../domain/acp";
import type { HarnessDefinition } from "../../../domain/harness";

export function promptAttachmentItems(
  items: readonly AcpItem[],
): Extract<AcpItem, { kind: "image" }>[] {
  let prompt: AcpItem | undefined;
  for (let index = items.length - 1; index >= 0; index -= 1) {
    const item = items[index];
    if (item?.kind === "text" && item.payload.role === "user") {
      prompt = item;
      break;
    }
  }
  const answer = items.find(
    (item) => item.kind === "text" && item.payload.role === "assistant",
  );
  return items.filter(
    (item): item is Extract<AcpItem, { kind: "image" }> =>
      item.kind === "image" &&
      item.payload.source === "user" &&
      !!prompt &&
      item.ordinal > prompt.ordinal &&
      item.ordinal < (answer?.ordinal ?? Infinity),
  );
}

const cancellableStates = new Set<TurnProjection["state"]>([
  "sending",
  "running",
  "waitingModel",
  "waitingTool",
  "waitingApproval",
  "waitingUser",
]);

function currentSession(page: AcpChatPage): AgentSessionProjection | null {
  if (!page.currentSessionId) return null;
  return (
    page.sessions.find((session) => session.id === page.currentSessionId) ??
    null
  );
}

const CODEX_ASK_NATIVE_APPROVAL_PROVEN = false;

export function permissionRequestsAvailable(
  page: AcpChatPage | null,
  selectedHarness?: HarnessDefinition | null,
): boolean {
  if (page?.currentSessionId) {
    return currentSession(page)?.permissionRequests === true;
  }
  return (
    selectedHarness?.id === "codex" &&
    selectedHarness.source === "builtIn" &&
    selectedHarness.transport === "acp" &&
    selectedHarness.installState === "available" &&
    selectedHarness.bindingId === "codex-app-server" &&
    selectedHarness.boundVersion === "0.146.0" &&
    selectedHarness.characterisation === "characterised" &&
    CODEX_ASK_NATIVE_APPROVAL_PROVEN
  );
}

export function latestRetryPrompt(page: AcpChatPage): {
  prompt: string;
  bindingId: AgentSessionProjection["bindingId"];
  modelId: AgentSessionProjection["modelId"];
} | null {
  const session = currentSession(page);
  const item = [...page.turns]
    .reverse()
    .flatMap((turn) => turn.items)
    .find(
      (candidate) =>
        candidate.kind === "text" && candidate.payload.role === "user",
    );
  return session && item?.kind === "text"
    ? {
        prompt: item.payload.text,
        bindingId: session.bindingId,
        modelId: session.modelId,
      }
    : null;
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

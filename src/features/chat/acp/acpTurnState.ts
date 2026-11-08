import type {
  AcpChatPage,
  AcpItem,
  AcpSessionAction,
  AgentSessionProjection,
  SageTurnId,
  TurnProjection,
} from "../../../domain/acp";
import { terminalItemLifecycles } from "./statusSemantics";

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

export function currentSession(
  page: AcpChatPage,
): AgentSessionProjection | null {
  if (!page.currentSessionId) return null;
  return (
    page.sessions.find((session) => session.id === page.currentSessionId) ??
    null
  );
}

export function latestRetryPrompt(page: AcpChatPage): {
  prompt: string;
  /** Present when the Turn ran a session action: Retry runs it again. */
  action?: AcpSessionAction;
  bindingId: AgentSessionProjection["bindingId"];
  modelId: AgentSessionProjection["modelId"];
} | null {
  const session = currentSession(page);
  const turn = page.turns.at(-1);
  const item = [...(turn?.items ?? [])]
    .reverse()
    .find(
      (candidate) =>
        candidate.origin === "primary" &&
        candidate.kind === "text" &&
        candidate.payload.role === "user",
    );
  return session &&
    turn?.sessionId === session.id &&
    !turn.unrecognizedAction &&
    item?.kind === "text"
    ? {
        prompt: item.payload.text,
        ...(turn.sessionAction ? { action: turn.sessionAction } : {}),
        bindingId: session.bindingId,
        modelId: session.modelId,
      }
    : null;
}

export function cancellableTurnId(page: AcpChatPage): SageTurnId | null {
  const session = currentSession(page);
  if (!session?.runtimeEpoch || !session.capabilities.cancel) return null;
  const active = page.turns.filter(
    (turn) =>
      turn.sessionId === session.id &&
      turn.runtimeEpoch === session.runtimeEpoch &&
      cancellableStates.has(turn.state),
  );
  return active.length === 1 ? active[0].id : null;
}

/** A Turn's top-level harness activities: no admitted parent in this Turn. */
export function rootActivities(turn: TurnProjection) {
  const activityIds = new Set(turn.activities.map((activity) => activity.id));
  const itemActivities = new Map(
    turn.items.map((item) => [item.id, item.activityId] as const),
  );
  return turn.activities.filter(
    (activity) =>
      !activity.parentActivityId ||
      !activityIds.has(activity.parentActivityId) ||
      itemActivities.get(activity.parentItemId) !== activity.parentActivityId,
  );
}

/**
 * A Turn's working and answer items in wire order: prompts, attachments,
 * settled thoughts and (once terminal) empty text are left out.
 */
export function chronologicalTurnItems(
  primary: readonly AcpItem[],
  attachmentIds: ReadonlySet<string>,
  terminal: boolean,
) {
  return primary
    .filter(
      (item) =>
        !attachmentIds.has(item.id) &&
        !(
          item.kind === "thoughtSummary" &&
          terminalItemLifecycles.has(item.lifecycle)
        ) &&
        !(item.kind === "text" && item.payload.role === "user") &&
        !(terminal && item.kind === "text" && !item.payload.text.trim()),
    )
    .sort(
      (left, right) =>
        (Number.isFinite(left.ordinal) ? left.ordinal : Infinity) -
        (Number.isFinite(right.ordinal) ? right.ordinal : Infinity),
    );
}

import type { AcpItem, AcpItemIdentity } from "../domain/acp";
import {
  safeNullableIdentifier,
  safeNullableText,
  safeText,
} from "./acpDesktopGuards";

const MAX_COLLABORATION_RECEIVERS = 32;

/** At most the receiver count of bounded texts, or none when any is not one. */
function boundedTexts(value: unknown) {
  return Array.isArray(value) &&
    value.length <= MAX_COLLABORATION_RECEIVERS &&
    value.every((text) => safeText(text, 512) !== null)
    ? (value as string[])
    : null;
}

/** Key and value pairs, each key a bounded text; none at all when absent. */
function keyedPairs(value: unknown, valid: (part: unknown) => boolean) {
  return !Array.isArray(value)
    ? []
    : value.length <= MAX_COLLABORATION_RECEIVERS &&
        value.every(
          (entry) =>
            Array.isArray(entry) &&
            entry.length === 2 &&
            boundedTexts([entry[0]]) !== null &&
            valid(entry[1]),
        )
      ? value
      : null;
}

type CollaborationItem = Extract<AcpItem, { kind: "collaboration" }>;

export function safeCollaborationItem(
  identity: AcpItemIdentity,
  payload: Record<string, unknown>,
): CollaborationItem | null {
  const activityId = safeNullableIdentifier(payload.activityId);
  const label = safeText(payload.label, 512);
  const summary = safeText(payload.summary, 16_384);
  const receiverLabels = boundedTexts(payload.receiverLabels);
  if (
    activityId === undefined ||
    (identity.origin !== "primary" &&
      (activityId === null || identity.activityId === null)) ||
    label === null ||
    summary === null ||
    !receiverLabels
  )
    return null;
  const operation = safeNullableText(payload.operation ?? null, 32);
  const agentKey = safeNullableText(payload.agentKey ?? null, 512);
  const agentKeys = Array.isArray(payload.agentKeys)
    ? boundedTexts(payload.agentKeys)
    : [];
  const agentStates = keyedPairs(
    payload.agentStates,
    (state) => safeText(state, 32) !== null,
  ) as [string, string][] | null;
  const untimedAgents =
    payload.untimedAgents === undefined
      ? undefined
      : boundedTexts(payload.untimedAgents);
  const currentAgents =
    payload.currentAgents === undefined
      ? undefined
      : boundedTexts(payload.currentAgents);
  return !agentKeys ||
    !agentStates ||
    untimedAgents === null ||
    currentAgents === null ||
    operation === undefined ||
    agentKey === undefined
    ? null
    : {
        ...identity,
        kind: "collaboration",
        toolState: null,
        payload: {
          activityId: activityId as CollaborationItem["payload"]["activityId"],
          label,
          summary,
          receiverLabels,
          operation,
          agentKey,
          agentKeys,
          agentStates,
          untimedAgents,
          currentAgents,
        },
      };
}

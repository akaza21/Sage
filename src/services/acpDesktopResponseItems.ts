import type { AcpItem, AcpItemIdentity, ToolPresentation } from "../domain/acp";
import {
  isOneOf,
  isRecord,
  safeCounter,
  safeIdentifier,
  safeNullableIdentifier,
  safeNullableText,
  safeOrdinal,
  safeText,
  safeTimestamp,
} from "./acpDesktopGuards";
import { admittedImageDataUrl } from "./acpDesktopResponseImage";
import {
  MAX_FILE_CHANGES,
  safeFileChange,
  safeToolPresentation,
} from "./acpDesktopResponsePayloads";

const MAX_PLAN_ENTRIES = 512;
const MAX_COLLABORATION_RECEIVERS = 32;

function safeItemIdentity(
  value: Record<string, unknown>,
): AcpItemIdentity | null {
  const id = safeIdentifier(value.id);
  const projectId = safeIdentifier(value.projectId);
  const chatId = safeIdentifier(value.chatId);
  const sessionId = safeIdentifier(value.sessionId);
  const turnId = safeIdentifier(value.turnId);
  const activityId = safeNullableIdentifier(value.activityId);
  const runtimeEpoch = safeCounter(value.runtimeEpoch, false);
  const revision = safeCounter(value.revision, true);
  const ordinal = safeOrdinal(value.ordinal);
  const createdAt = safeTimestamp(value.createdAt);
  const updatedAt = safeTimestamp(value.updatedAt);
  const terminalAt =
    value.terminalAt === null ? null : safeTimestamp(value.terminalAt);
  if (
    !id ||
    !projectId ||
    !chatId ||
    !sessionId ||
    !turnId ||
    !runtimeEpoch ||
    !revision ||
    activityId === undefined ||
    ordinal === null ||
    !isOneOf(value.origin, ["primary", "harnessActivity"] as const) ||
    !isOneOf(value.lifecycle, [
      "started",
      "streaming",
      "completed",
      "failed",
      "cancelled",
      "unsupported",
    ] as const) ||
    createdAt === null ||
    updatedAt === null ||
    (terminalAt === null && value.terminalAt !== null)
  )
    return null;
  return {
    id: id as AcpItemIdentity["id"],
    projectId: projectId as AcpItemIdentity["projectId"],
    chatId: chatId as AcpItemIdentity["chatId"],
    sessionId: sessionId as AcpItemIdentity["sessionId"],
    turnId: turnId as AcpItemIdentity["turnId"],
    runtimeEpoch: runtimeEpoch as AcpItemIdentity["runtimeEpoch"],
    ordinal,
    origin: value.origin,
    activityId: activityId as AcpItemIdentity["activityId"],
    lifecycle: value.lifecycle,
    revision: revision as AcpItemIdentity["revision"],
    createdAt,
    updatedAt,
    terminalAt,
  };
}

export function safeItem(value: unknown): AcpItem | null {
  if (!isRecord(value) || !isRecord(value.payload)) return null;
  const identity = safeItemIdentity(value);
  if (!identity) return null;
  const { payload } = value;
  switch (value.kind) {
    case "text": {
      const text = safeText(payload.text, 262_144);
      return value.toolState === null &&
        text !== null &&
        isOneOf(payload.role, ["user", "assistant", "system"] as const)
        ? {
            ...identity,
            kind: "text",
            toolState: null,
            payload: { role: payload.role, text },
          }
        : null;
    }
    case "thoughtSummary": {
      const summary = safeText(payload.summary, 262_144);
      return value.toolState === null && summary !== null
        ? {
            ...identity,
            kind: "thoughtSummary",
            toolState: null,
            payload: { summary },
          }
        : null;
    }
    case "compactionNotice":
      return value.toolState === null && Object.keys(payload).length === 0
        ? {
            ...identity,
            kind: "compactionNotice",
            toolState: null,
            payload: {},
          }
        : null;
    case "plan": {
      if (
        value.toolState !== null ||
        !Array.isArray(payload.entries) ||
        payload.entries.length > MAX_PLAN_ENTRIES
      )
        return null;
      const entries = payload.entries.map((entry) => {
        if (!isRecord(entry)) return null;
        const label = safeText(entry.label, 512);
        return label !== null &&
          isOneOf(entry.state, [
            "pending",
            "inProgress",
            "completed",
            "failed",
          ] as const)
          ? { label, state: entry.state }
          : null;
      });
      return entries.some((entry) => !entry)
        ? null
        : {
            ...identity,
            kind: "plan",
            toolState: null,
            payload: {
              entries: entries as {
                label: string;
                state: "pending" | "inProgress" | "completed" | "failed";
              }[],
            },
          };
    }
    case "toolCall": {
      const presentation = safeToolPresentation(payload.presentation);
      const inputSummary = safeNullableText(payload.inputSummary, 65_536);
      const outputSummary =
        payload.outputSummary === undefined
          ? null
          : safeNullableText(payload.outputSummary, 65_536);
      return !isOneOf(value.toolState, [
        "pending",
        "inProgress",
        "waitingForApproval",
        "completed",
        "failed",
        "declined",
        "cancelled",
      ] as const) ||
        !presentation ||
        inputSummary === undefined ||
        outputSummary === undefined
        ? null
        : {
            ...identity,
            kind: "toolCall",
            toolState: value.toolState,
            payload: { presentation, inputSummary, outputSummary },
          };
    }
    case "fileChangeDiff": {
      if (
        value.toolState !== null ||
        !Array.isArray(payload.changes) ||
        payload.changes.length > MAX_FILE_CHANGES ||
        typeof payload.truncated !== "boolean"
      )
        return null;
      const changes = payload.changes.map(safeFileChange);
      return changes.some((change) => !change)
        ? null
        : {
            ...identity,
            kind: "fileChangeDiff",
            toolState: null,
            payload: {
              changes: changes as NonNullable<(typeof changes)[number]>[],
              truncated: payload.truncated,
            },
          };
    }
    case "image": {
      const image = admittedImageDataUrl(payload.mediaType, payload.dataUrl);
      return value.toolState === null && image
        ? {
            ...identity,
            kind: "image",
            toolState: null,
            payload: image,
          }
        : null;
    }
    case "permissionRequest": {
      const requestId = safeIdentifier(payload.requestId);
      const label = safeText(payload.label, 512);
      const summary = safeText(payload.summary, 65_536);
      const offeredOptionKinds =
        Array.isArray(payload.offeredOptionKinds) &&
        payload.offeredOptionKinds.length <= 8 &&
        payload.offeredOptionKinds.every((kind) =>
          isOneOf(kind, [
            "allow_once",
            "allow_always",
            "reject_once",
            "reject_always",
          ] as const),
        )
          ? (payload.offeredOptionKinds as Array<
              "allow_once" | "allow_always" | "reject_once" | "reject_always"
            >)
          : undefined;
      const source = isOneOf(payload.source, [
        "harness",
        "browser",
        "browserDeveloper",
      ] as const)
        ? payload.source
        : undefined;
      return !isOneOf(value.toolState, [
        "pending",
        "inProgress",
        "waitingForApproval",
        "completed",
        "failed",
        "declined",
        "cancelled",
      ] as const) ||
        !requestId ||
        label === null ||
        summary === null ||
        !isOneOf(payload.state, [
          "received",
          "asked",
          "allowPending",
          "allowCommitted",
          "declinePending",
          "declineCommitted",
          "responseQueued",
          "resolvedAllowed",
          "resolvedDeclined",
          "cancelled",
        ] as const)
        ? null
        : {
            ...identity,
            kind: "permissionRequest",
            toolState: value.toolState,
            payload: {
              requestId: requestId as Extract<
                AcpItem,
                { kind: "permissionRequest" }
              >["payload"]["requestId"],
              state: payload.state,
              label,
              summary,
              source,
              offeredOptionKinds,
              category:
                typeof payload.category === "string"
                  ? (payload.category as ToolPresentation["kind"])
                  : undefined,
              reason: safeNullableText(payload.reason ?? null, 2048) ?? null,
            },
          };
    }
    case "error": {
      const code = safeText(payload.code, 128);
      const message = safeText(payload.message, 8192);
      return value.toolState === null &&
        code !== null &&
        message !== null &&
        typeof payload.recoverable === "boolean"
        ? {
            ...identity,
            kind: "error",
            toolState: null,
            payload: {
              code,
              message,
              recoverable: payload.recoverable,
              ...(payload.kind === "attempt"
                ? { kind: "attempt" as const }
                : {}),
            },
          }
        : null;
    }
    case "unsupported": {
      const category = safeText(payload.category, 128);
      const summary = safeText(payload.summary, 16_384);
      return value.toolState === null &&
        category !== null &&
        summary !== null &&
        typeof payload.truncated === "boolean"
        ? {
            ...identity,
            kind: "unsupported",
            toolState: null,
            payload: { category, summary, truncated: payload.truncated },
          }
        : null;
    }
    case "collaboration": {
      const activityId = safeNullableIdentifier(payload.activityId);
      const label = safeText(payload.label, 512);
      const summary = safeText(payload.summary, 16_384);
      if (
        value.toolState !== null ||
        activityId === undefined ||
        (identity.origin !== "primary" &&
          (activityId === null || identity.activityId === null)) ||
        label === null ||
        summary === null ||
        !Array.isArray(payload.receiverLabels) ||
        payload.receiverLabels.length > MAX_COLLABORATION_RECEIVERS
      )
        return null;
      const receiverLabels = payload.receiverLabels.map((receiver) =>
        safeText(receiver, 512),
      );
      const operation =
        payload.operation == null ? null : safeText(payload.operation, 32);
      const agentKey =
        payload.agentKey == null ? null : safeText(payload.agentKey, 512);
      const agentKeys = Array.isArray(payload.agentKeys)
        ? payload.agentKeys.map((key) => safeText(key, 512))
        : [];
      const agentStates = Array.isArray(payload.agentStates)
        ? payload.agentStates.map((entry) =>
            Array.isArray(entry) && entry.length === 2
              ? [safeText(entry[0], 512), safeText(entry[1], 32)]
              : null,
          )
        : [];
      return receiverLabels.some((receiver) => receiver === null) ||
        (payload.operation != null && operation === null) ||
        (payload.agentKey != null && agentKey === null) ||
        agentKeys.length > MAX_COLLABORATION_RECEIVERS ||
        agentKeys.some((key) => key === null) ||
        agentStates.length > MAX_COLLABORATION_RECEIVERS ||
        agentStates.some(
          (entry) => entry === null || entry.some((part) => part === null),
        )
        ? null
        : {
            ...identity,
            kind: "collaboration",
            toolState: null,
            payload: {
              activityId: activityId as Extract<
                AcpItem,
                { kind: "collaboration" }
              >["payload"]["activityId"],
              label,
              summary,
              receiverLabels: receiverLabels as string[],
              operation,
              agentKey,
              agentKeys: agentKeys as string[],
              agentStates: agentStates as [string, string][],
            },
          };
    }
    default:
      return null;
  }
}

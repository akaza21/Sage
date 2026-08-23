import type {
  AcpItem,
  AcpItemIdentity,
  FileChangeProjection,
  HarnessActivityProjection,
  SafeResourceLocation,
  ToolPresentation,
  TurnProjection,
} from "../domain/acp";
import type { ResourceRef } from "../domain/workbench";
import {
  isOneOf,
  isRecord,
  safeCounter,
  safeIdentifier,
  safeNullableCounter,
  safeNullableIdentifier,
  safeNullableText,
  safeOrdinal,
  safeText,
  safeTimestamp,
} from "./acpDesktopResponses";

const MAX_PLAN_ENTRIES = 512;
const MAX_FILE_CHANGES = 64;
const MAX_TOOL_RESOURCES = 64;
const MAX_COLLABORATION_RECEIVERS = 32;

function safeResourceRef(value: unknown): ResourceRef | null {
  if (!isRecord(value)) return null;
  const rootId = safeIdentifier(value.rootId);
  const relativePath = safeText(value.relativePath, 16_384);
  return rootId && relativePath !== null ? { rootId, relativePath } : null;
}

function safeResourceLocation(value: unknown): SafeResourceLocation | null {
  if (!isRecord(value)) return null;
  if (value.kind === "resource") {
    const resource = safeResourceRef(value.resource);
    return resource ? { kind: "resource", resource } : null;
  }
  const label = safeText(value.label, 512);
  if (
    label === null ||
    !isOneOf(value.kind, ["unavailable"] as const) ||
    !isOneOf(value.reason, [
      "ambiguousRoot",
      "foreign",
      "missing",
      "noncanonical",
      "unsupported",
    ] as const)
  ) {
    return null;
  }
  return { kind: "unavailable", reason: value.reason, label };
}

function safeNullableBoundedNumber(value: unknown): number | null | undefined {
  if (value === null) return null;
  return Number.isSafeInteger(value) && value >= 0 && value <= 1_000_000
    ? value
    : undefined;
}

function safeNullableNonnegativeInteger(
  value: unknown,
): number | null | undefined {
  if (value === null) return null;
  return Number.isSafeInteger(value) && value >= 0 ? value : undefined;
}

function safeFileChange(value: unknown): FileChangeProjection | null {
  if (!isRecord(value)) return null;
  const resource = safeResourceLocation(value.resource);
  const originalResource =
    value.originalResource === null
      ? null
      : (safeResourceLocation(value.originalResource) ?? undefined);
  const summary = safeText(value.summary, 65_536);
  const additions = safeNullableBoundedNumber(value.additions);
  const deletions = safeNullableBoundedNumber(value.deletions);
  if (
    !resource ||
    originalResource === undefined ||
    summary === null ||
    additions === undefined ||
    deletions === undefined ||
    !isOneOf(value.operation, [
      "created",
      "updated",
      "deleted",
      "renamed",
      "unknown",
    ] as const)
  ) {
    return null;
  }
  return {
    resource,
    operation: value.operation,
    originalResource,
    summary,
    additions,
    deletions,
  };
}

function safeToolPresentation(value: unknown): ToolPresentation | null {
  if (!isRecord(value)) return null;
  const label = safeText(value.label, 512);
  if (label === null) return null;
  switch (value.kind) {
    case "generic":
    case "command":
    case "web": {
      const summary = safeNullableText(value.summary, 16_384);
      return summary === undefined
        ? null
        : { kind: value.kind, label, summary };
    }
    case "file":
    case "diff": {
      if (
        !Array.isArray(value.resources) ||
        value.resources.length > MAX_TOOL_RESOURCES
      ) {
        return null;
      }
      const resources = value.resources.map(safeResourceLocation);
      return resources.some((resource) => !resource)
        ? null
        : {
            kind: value.kind,
            label,
            resources: resources as SafeResourceLocation[],
          };
    }
    case "search": {
      const querySummary = safeText(value.querySummary, 16_384);
      if (
        querySummary === null ||
        !Array.isArray(value.resources) ||
        value.resources.length > MAX_TOOL_RESOURCES
      ) {
        return null;
      }
      const resources = value.resources.map(safeResourceRef);
      return resources.some((resource) => !resource)
        ? null
        : {
            kind: "search",
            label,
            querySummary,
            resources: resources as ResourceRef[],
          };
    }
    case "mcp": {
      const serverLabel = safeText(value.serverLabel, 512);
      const toolLabel = safeText(value.toolLabel, 512);
      return serverLabel === null || toolLabel === null
        ? null
        : { kind: "mcp", label, serverLabel, toolLabel };
    }
    case "subagent": {
      const activityId = safeNullableIdentifier(value.activityId);
      return activityId === undefined
        ? null
        : {
            kind: "subagent",
            label,
            activityId: activityId as Extract<
              ToolPresentation,
              { kind: "subagent" }
            >["activityId"],
          };
    }
    default:
      return null;
  }
}

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
  const revision = safeCounter(value.revision, false);
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
        inputSummary === undefined
        ? null
        : {
            ...identity,
            kind: "toolCall",
            toolState: value.toolState,
            payload: { presentation, inputSummary },
          };
    }
    case "toolUpdateResult": {
      const callItemId = safeNullableIdentifier(payload.callItemId);
      const presentation = safeToolPresentation(payload.presentation);
      const outputSummary = safeNullableText(payload.outputSummary, 65_536);
      return !isOneOf(value.toolState, [
        "pending",
        "inProgress",
        "waitingForApproval",
        "completed",
        "failed",
        "declined",
        "cancelled",
      ] as const) ||
        callItemId === undefined ||
        !presentation ||
        outputSummary === undefined
        ? null
        : {
            ...identity,
            kind: "toolUpdateResult",
            toolState: value.toolState,
            payload: {
              callItemId: callItemId as Extract<
                AcpItem,
                { kind: "toolUpdateResult" }
              >["payload"]["callItemId"],
              presentation,
              outputSummary,
            },
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
              changes: changes as FileChangeProjection[],
              truncated: payload.truncated,
            },
          };
    }
    case "usage": {
      const counters = [
        payload.inputTokens,
        payload.outputTokens,
        payload.cachedInputTokens,
        payload.totalTokens,
      ].map(safeNullableNonnegativeInteger);
      return value.toolState !== null ||
        counters.some((counter) => counter === undefined)
        ? null
        : {
            ...identity,
            kind: "usage",
            toolState: null,
            payload: {
              inputTokens: counters[0] as number | null,
              outputTokens: counters[1] as number | null,
              cachedInputTokens: counters[2] as number | null,
              totalTokens: counters[3] as number | null,
            },
          };
    }
    case "permissionRequest": {
      const requestId = safeIdentifier(payload.requestId);
      const label = safeText(payload.label, 512);
      const summary = safeText(payload.summary, 65_536);
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
            payload: { code, message, recoverable: payload.recoverable },
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
      const activityId = safeIdentifier(payload.activityId);
      const label = safeText(payload.label, 512);
      const summary = safeText(payload.summary, 16_384);
      if (
        value.toolState !== null ||
        !activityId ||
        label === null ||
        summary === null ||
        !Array.isArray(payload.receiverLabels) ||
        payload.receiverLabels.length > MAX_COLLABORATION_RECEIVERS
      )
        return null;
      const receiverLabels = payload.receiverLabels.map((receiver) =>
        safeText(receiver, 512),
      );
      return receiverLabels.some((receiver) => receiver === null)
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
            },
          };
    }
    default:
      return null;
  }
}

export function safeActivity(value: unknown): HarnessActivityProjection | null {
  if (!isRecord(value) || !isRecord(value.provenance)) return null;
  const id = safeIdentifier(value.id);
  const turnId = safeIdentifier(value.turnId);
  const parentActivityId = safeNullableIdentifier(value.parentActivityId);
  const parentItemId = safeIdentifier(value.parentItemId);
  const revision = safeCounter(value.revision, false);
  const externalKind = safeText(value.provenance.externalKind, 128);
  const extensionSchemaId = safeNullableIdentifier(
    value.provenance.extensionSchemaId,
  );
  const safeLabel = safeNullableText(value.provenance.safeLabel, 512);
  const depth = safeOrdinal(value.depth);
  if (
    !id ||
    !turnId ||
    parentActivityId === undefined ||
    !parentItemId ||
    !isOneOf(value.role, ["root", "fork", "child", "unknown"] as const) ||
    !isOneOf(value.lifecycle, [
      "observed",
      "active",
      "completed",
      "failed",
      "cancelled",
      "unsupported",
    ] as const) ||
    depth === null ||
    depth > 255 ||
    value.provenance.schemaVersion !== 1 ||
    !isOneOf(value.provenance.sourceKind, [
      "tool",
      "extension",
      "collaboration",
      "unknown",
    ] as const) ||
    !externalKind ||
    extensionSchemaId === undefined ||
    safeLabel === undefined ||
    !revision
  )
    return null;
  return {
    id: id as HarnessActivityProjection["id"],
    turnId: turnId as HarnessActivityProjection["turnId"],
    parentActivityId:
      parentActivityId as HarnessActivityProjection["parentActivityId"],
    parentItemId: parentItemId as HarnessActivityProjection["parentItemId"],
    role: value.role,
    lifecycle: value.lifecycle,
    depth,
    provenance: {
      schemaVersion: 1,
      sourceKind: value.provenance.sourceKind,
      externalKind,
      extensionSchemaId,
      safeLabel,
    },
    revision: revision as HarnessActivityProjection["revision"],
  };
}

export function safeTurn(value: unknown): TurnProjection | null {
  if (
    !isRecord(value) ||
    !Array.isArray(value.items) ||
    !Array.isArray(value.activities) ||
    value.items.length > 512 ||
    value.activities.length > 128
  )
    return null;
  const id = safeIdentifier(value.id);
  const sessionId = safeIdentifier(value.sessionId);
  const runtimeEpoch = safeCounter(value.runtimeEpoch, false);
  const revision = safeCounter(value.revision, false);
  const ordinal = safeOrdinal(value.ordinal);
  const createdAt = safeTimestamp(value.createdAt);
  const updatedAt = safeTimestamp(value.updatedAt);
  const terminalAt =
    value.terminalAt === null ? null : safeTimestamp(value.terminalAt);
  if (
    !id ||
    !sessionId ||
    !runtimeEpoch ||
    ordinal === null ||
    !isOneOf(value.state, [
      "queued",
      "sending",
      "running",
      "waitingModel",
      "waitingTool",
      "waitingApproval",
      "waitingUser",
      "cancelling",
      "completed",
      "interrupted",
      "cancelled",
      "failed",
      "deliveryUnknown",
    ] as const) ||
    !revision ||
    createdAt === null ||
    updatedAt === null ||
    (terminalAt === null && value.terminalAt !== null)
  )
    return null;
  const items = value.items.map(safeItem);
  const activities = value.activities.map(safeActivity);
  return items.some((item) => !item) || activities.some((activity) => !activity)
    ? null
    : {
        id: id as TurnProjection["id"],
        sessionId: sessionId as TurnProjection["sessionId"],
        runtimeEpoch: runtimeEpoch as TurnProjection["runtimeEpoch"],
        ordinal,
        state: value.state,
        revision: revision as TurnProjection["revision"],
        createdAt,
        updatedAt,
        terminalAt,
        items: items as AcpItem[],
        activities: activities as HarnessActivityProjection[],
      };
}

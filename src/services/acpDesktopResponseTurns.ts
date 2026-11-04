import type {
  AcpItem,
  HarnessActivityProjection,
  TurnProjection,
} from "../domain/acp";
import type { BrowserOperationProjection } from "../domain/browserOperation";
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
import { safeItem } from "./acpDesktopResponseItems";
import { validateAttachedContexts } from "../domain/attachedContext";

function safeBrowserOperation(
  value: unknown,
): BrowserOperationProjection | null {
  if (!isRecord(value)) return null;
  const projectId = safeIdentifier(value.projectId);
  const chatId = safeIdentifier(value.chatId);
  const sessionId = safeIdentifier(value.sessionId);
  const turnId = safeIdentifier(value.turnId);
  const tabId = safeText(value.tabId, 128);
  const operationId = safeIdentifier(value.operationId);
  const origin = safeText(value.origin, 512);
  const summary = safeText(value.summary, 512);
  const resultSummary =
    value.resultSummary === undefined
      ? undefined
      : safeText(value.resultSummary, 512);
  const createdAt = safeTimestamp(value.createdAt);
  const updatedAt = safeTimestamp(value.updatedAt);
  const operationKind = safeText(value.operation, 128);
  const knownOperation = isOneOf(operationKind, [
    "openTab",
    "navigate",
    "snapshotPage",
    "act",
    "waitFor",
    "screenshot",
    "readConsole",
    "readNetwork",
  ] as const);
  if (
    !projectId ||
    !chatId ||
    !sessionId ||
    !turnId ||
    !tabId ||
    !operationId ||
    !origin ||
    !summary ||
    resultSummary === null ||
    createdAt === null ||
    updatedAt === null ||
    !operationKind ||
    !isOneOf(value.phase, [
      "started",
      "completed",
      "denied",
      "failed",
      "outcomeUnknown",
    ] as const)
  )
    return null;
  return {
    projectId,
    chatId,
    sessionId,
    turnId,
    tabId,
    operationId,
    operation: knownOperation ? operationKind : "unsupported",
    phase: knownOperation ? value.phase : "failed",
    origin,
    summary: knownOperation ? summary : "Unsupported browser operation",
    ...(knownOperation
      ? resultSummary === undefined
        ? {}
        : { resultSummary }
      : {
          resultSummary:
            "This browser operation is not supported by this version of Sage.",
        }),
    createdAt,
    updatedAt,
  } as BrowserOperationProjection;
}

function safeUsage(value: unknown): TurnProjection["usage"] | undefined | null {
  if (value === undefined) return undefined;
  if (!isRecord(value)) return null;
  const inputTokens = safeOrdinal(value.inputTokens);
  const outputTokens = safeOrdinal(value.outputTokens);
  const totalTokens = safeOrdinal(value.totalTokens);
  const contextWindow =
    value.contextWindow === undefined
      ? undefined
      : safeOrdinal(value.contextWindow);
  if (
    inputTokens === null ||
    outputTokens === null ||
    totalTokens === null ||
    contextWindow === null ||
    (contextWindow !== undefined && contextWindow > 0xffff_ffff)
  )
    return null;
  return {
    inputTokens,
    outputTokens,
    totalTokens,
    ...(contextWindow === undefined ? {} : { contextWindow }),
  };
}

export function safeActivity(value: unknown): HarnessActivityProjection | null {
  if (!isRecord(value) || !isRecord(value.provenance)) return null;
  const id = safeIdentifier(value.id);
  const turnId = safeIdentifier(value.turnId);
  const parentActivityId = safeNullableIdentifier(value.parentActivityId);
  const parentItemId = safeIdentifier(value.parentItemId);
  const revision = safeCounter(value.revision, true);
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

function safeTerminalReason(
  value: unknown,
): TurnProjection["terminalReason"] | undefined | null {
  if (value === undefined) return undefined;
  if (!isRecord(value)) return null;
  if (
    !isOneOf(value.kind, [
      "authRequired",
      "harnessExited",
      "protocolError",
      "cancelledByUser",
      "refusedVersion",
      "timeout",
      "other",
    ])
  )
    return null;
  const message = safeTerminalText(value.message);
  const remediation = safeTerminalText(value.remediation);
  if (message === null || remediation === null) return null;
  return {
    kind: value.kind,
    ...(message === undefined ? {} : { message }),
    ...(remediation === undefined ? {} : { remediation }),
  };
}

function safeTerminalText(value: unknown): string | undefined | null {
  if (value === undefined) return undefined;
  return typeof value === "string" && value.length <= 8192 ? value : null;
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
  const rawContext = value.attachedContext ?? [];
  if (!Array.isArray(rawContext)) return null;
  let attachedContext: TurnProjection["attachedContext"];
  try {
    // Bounded like the composer: at most eight entries, each UTF-8 bounded.
    attachedContext = validateAttachedContexts(rawContext);
  } catch {
    return null;
  }
  const sessionId = safeIdentifier(value.sessionId);
  const runtimeEpoch = safeCounter(value.runtimeEpoch, false);
  const revision = safeCounter(value.revision, true);
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
  const usage = safeUsage(value.usage);
  if (usage === null) return null;
  const terminalReason = safeTerminalReason(value.terminalReason);
  if (terminalReason === null) return null;
  const items = value.items.map(safeItem);
  const activities = value.activities.map(safeActivity);
  const rawBrowserOperations = value.browserOperations ?? [];
  if (!Array.isArray(rawBrowserOperations) || rawBrowserOperations.length > 32)
    return null;
  const browserOperations = rawBrowserOperations.map(safeBrowserOperation);
  const browserOperationsCollapsed = safeOrdinal(
    value.browserOperationsCollapsed ?? 0,
  );
  return items.some((item) => !item) ||
    activities.some((activity) => !activity) ||
    browserOperations.some((operation) => !operation) ||
    browserOperationsCollapsed === null
    ? null
    : {
        ...(attachedContext.length ? { attachedContext } : {}),
        id: id as TurnProjection["id"],
        sessionId: sessionId as TurnProjection["sessionId"],
        runtimeEpoch: runtimeEpoch as TurnProjection["runtimeEpoch"],
        ordinal,
        state: value.state,
        revision: revision as TurnProjection["revision"],
        createdAt,
        updatedAt,
        terminalAt,
        ...(usage === undefined ? {} : { usage }),
        ...(terminalReason === undefined ? {} : { terminalReason }),
        items: items as AcpItem[],
        activities: activities as HarnessActivityProjection[],
        browserOperations: browserOperations as BrowserOperationProjection[],
        browserOperationsCollapsed,
      };
}

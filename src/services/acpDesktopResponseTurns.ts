import type {
  AcpItem,
  HarnessActivityProjection,
  TurnProjection,
} from "../domain/acp";
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
} from "./acpDesktopResponses";
import { safeItem } from "./acpDesktopResponseItems";

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

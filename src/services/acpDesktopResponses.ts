import type {
  AcpChatPage,
  AcpMutationResult,
  AgentSessionProjection,
  CodexDiscoveryModel,
  CodexDiscoveryResponse,
} from "../domain/acp";
import {
  boundedIdentifier as safeIdentifier,
  decimalU64Counter,
  isRecord,
} from "../domain/guards";
import { utf8ByteLength } from "../domain/text";
import { safeTurn } from "./acpDesktopResponseTurns";

const ACP_PAGE_LIMITS = {
  sessions: 32,
  turns: 20,
  items: 256,
  activities: 128,
} as const;

export function safeCounter(value: unknown, allowZero: boolean): string | null {
  return decimalU64Counter(value, { allowZero });
}

export { isRecord, safeIdentifier };

export function isOneOf<const Value extends string>(
  value: unknown,
  allowed: readonly Value[],
): value is Value {
  return typeof value === "string" && allowed.includes(value as Value);
}

export function safeText(value: unknown, maximumBytes: number): string | null {
  return typeof value === "string" && utf8ByteLength(value) <= maximumBytes
    ? value
    : null;
}

export function safeMessage(value: unknown): string | null {
  if (typeof value !== "string" || value.length === 0 || value.length > 8192) {
    return null;
  }
  return utf8ByteLength(value) <= 8192 ? value : null;
}

export function safeOrdinal(value: unknown): number | null {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0
    ? value
    : null;
}

export function safeTimestamp(value: unknown): number | null {
  return typeof value === "number" && Number.isSafeInteger(value)
    ? value
    : null;
}

export function safeNullableIdentifier(
  value: unknown,
): string | null | undefined {
  return value === null ? null : (safeIdentifier(value) ?? undefined);
}

export function safeNullableCounter(
  value: unknown,
  allowZero: boolean,
): string | null | undefined {
  return value === null ? null : (safeCounter(value, allowZero) ?? undefined);
}

export function safeNullableText(
  value: unknown,
  maximumBytes: number,
): string | null | undefined {
  return value === null ? null : (safeText(value, maximumBytes) ?? undefined);
}

function safeCapabilities(
  value: unknown,
): AgentSessionProjection["capabilities"] | null {
  if (!isRecord(value)) return null;
  const protocolVersion = safeText(value.protocolVersion, 64);
  const extensionSchemaId = safeNullableIdentifier(
    value.multiRootExtensionSchemaId,
  );
  const flags = [
    value.newSession,
    value.resumeSession,
    value.loadSession,
    value.closeSession,
    value.prompt,
    value.cancel,
    value.nativeSessionFork,
    value.activeTurnSteer,
    value.childConversationProjection,
    value.childTranscriptRead,
    value.collaborationEventProjection,
  ];
  const modelCount = safeOrdinal(value.modelCount);
  const configCount = safeOrdinal(value.configCount);
  if (
    value.schemaVersion !== 1 ||
    !protocolVersion ||
    extensionSchemaId === undefined ||
    flags.some((flag) => typeof flag !== "boolean") ||
    modelCount === null ||
    modelCount > 128 ||
    configCount === null ||
    configCount > 128
  )
    return null;
  return {
    schemaVersion: 1,
    protocolVersion,
    newSession: value.newSession,
    resumeSession: value.resumeSession,
    loadSession: value.loadSession,
    closeSession: value.closeSession,
    prompt: value.prompt,
    cancel: value.cancel,
    nativeSessionFork: value.nativeSessionFork,
    activeTurnSteer: value.activeTurnSteer,
    childConversationProjection: value.childConversationProjection,
    childTranscriptRead: value.childTranscriptRead,
    collaborationEventProjection: value.collaborationEventProjection,
    multiRootExtensionSchemaId: extensionSchemaId,
    modelCount,
    configCount,
  } as AgentSessionProjection["capabilities"];
}

import { decodeSessionConfig } from "../domain/acpSessionConfig";

function safeSession(value: unknown): AgentSessionProjection | null {
  if (!isRecord(value)) return null;
  const id = safeIdentifier(value.id);
  const bindingId = safeIdentifier(value.bindingId);
  const modelId = safeNullableIdentifier(value.modelId);
  const runtimeEpoch = safeNullableCounter(value.runtimeEpoch, false);
  const nextRuntimeEpoch = safeNullableCounter(value.nextRuntimeEpoch, false);
  const ordinal = safeOrdinal(value.ordinal);
  const revision = safeCounter(value.revision, true);
  const projectionRevision = safeCounter(value.projectionRevision, false);
  const capabilities = safeCapabilities(value.capabilities);
  if (
    !id ||
    ordinal === null ||
    !bindingId ||
    modelId === undefined ||
    !isOneOf(value.state, [
      "created",
      "starting",
      "ready",
      "active",
      "recovering",
      "closing",
      "closed",
      "failed",
      "incompatible",
    ] as const) ||
    !isOneOf(value.residency, ["hot", "cold"] as const) ||
    runtimeEpoch === undefined ||
    nextRuntimeEpoch === undefined ||
    !capabilities ||
    typeof value.permissionRequests !== "boolean" ||
    !revision ||
    !projectionRevision ||
    typeof value.legacy !== "boolean"
  )
    return null;
  return {
    id: id as AgentSessionProjection["id"],
    ordinal,
    bindingId: bindingId as AgentSessionProjection["bindingId"],
    modelId: modelId as AgentSessionProjection["modelId"],
    state: value.state,
    residency: value.residency,
    runtimeEpoch: runtimeEpoch as AgentSessionProjection["runtimeEpoch"],
    nextRuntimeEpoch:
      nextRuntimeEpoch as AgentSessionProjection["nextRuntimeEpoch"],
    capabilities,
    permissionRequests: value.permissionRequests,
    revision: revision as AgentSessionProjection["revision"],
    projectionRevision:
      projectionRevision as AgentSessionProjection["projectionRevision"],
    legacy: value.legacy,
  };
}

export function safeAcpMutationResponse(
  value: unknown,
): AcpMutationResult | null {
  if (!isRecord(value) || !isRecord(value.projection)) return null;
  const projection = value.projection;
  const projectId = safeIdentifier(projection.projectId);
  const chatId = safeIdentifier(projection.chatId);
  const title = safeText(projection.title, 8192);
  const projectionRevision = safeCounter(projection.projectionRevision, true);
  const currentSessionId =
    projection.currentSessionId === null
      ? null
      : safeIdentifier(projection.currentSessionId);
  const nextCursor =
    projection.nextCursor === null
      ? null
      : safeIdentifier(projection.nextCursor);
  if (
    !projectId ||
    !chatId ||
    title === null ||
    !projectionRevision ||
    (projection.currentSessionId !== null && !currentSessionId) ||
    !Array.isArray(projection.sessions) ||
    !Array.isArray(projection.turns) ||
    projection.sessions.length > ACP_PAGE_LIMITS.sessions ||
    projection.turns.length > ACP_PAGE_LIMITS.turns ||
    (projection.nextCursor !== null && !nextCursor)
  )
    return null;
  const sessions = projection.sessions.map(safeSession);
  const turns = projection.turns.map(safeTurn);
  if (sessions.some((session) => !session) || turns.some((turn) => !turn))
    return null;
  const safeSessions = sessions as AgentSessionProjection[];
  const sessionConfig =
    projection.sessionConfig == null
      ? null
      : decodeSessionConfig(projection.sessionConfig);
  if (projection.sessionConfig != null && !sessionConfig) return null;
  const configOwner = safeSessions.find(
    (session) => session.id === currentSessionId,
  );
  if (
    sessionConfig &&
    (sessionConfig.sageAgentSessionId !== currentSessionId ||
      sessionConfig.runtimeEpoch !== configOwner?.runtimeEpoch ||
      configOwner.residency !== "hot" ||
      !["ready", "active"].includes(configOwner.state))
  )
    return null;
  if (
    safeSessions.some(
      (session) =>
        session.capabilities.configCount !==
        (session.id === currentSessionId && !sessionConfig?.removed
          ? (sessionConfig?.configOptions.length ?? 0)
          : 0),
    )
  )
    return null;
  const safeTurns = turns.filter(
    (turn): turn is NonNullable<typeof turn> => turn !== null,
  );
  const itemCount = safeTurns.reduce(
    (count, turn) => count + turn.items.length,
    0,
  );
  const activityCount = safeTurns.reduce(
    (count, turn) => count + turn.activities.length,
    0,
  );
  const sessionIds = new Set(safeSessions.map((session) => session.id));
  if (
    itemCount > ACP_PAGE_LIMITS.items ||
    activityCount > ACP_PAGE_LIMITS.activities ||
    (currentSessionId !== null &&
      !sessionIds.has(currentSessionId as AgentSessionProjection["id"])) ||
    safeTurns.some(
      (turn) =>
        !sessionIds.has(turn.sessionId) ||
        turn.items.some(
          (item) =>
            item.projectId !== projectId ||
            item.chatId !== chatId ||
            item.sessionId !== turn.sessionId ||
            item.turnId !== turn.id,
        ) ||
        turn.activities.some((activity) => activity.turnId !== turn.id),
    )
  )
    return null;
  return {
    projection: {
      projectId: projectId as AcpChatPage["projectId"],
      chatId: chatId as AcpChatPage["chatId"],
      title,
      projectionRevision:
        projectionRevision as AcpChatPage["projectionRevision"],
      currentSessionId: currentSessionId as AcpChatPage["currentSessionId"],
      sessions: safeSessions,
      sessionConfig,
      turns: safeTurns,
      nextCursor: nextCursor as AcpChatPage["nextCursor"],
    },
  };
}

export function safeCodexDiscovery(
  value: unknown,
): CodexDiscoveryResponse | null {
  if (!isRecord(value) || !Array.isArray(value.models)) return null;
  const projectId = safeIdentifier(value.projectId);
  const projectGeneration = safeCounter(value.projectGeneration, true);
  const rootId = safeIdentifier(value.rootId);
  const bindingId = safeIdentifier(value.bindingId);
  const runtimeEpoch = safeCounter(value.runtimeEpoch, false);
  if (
    !projectId ||
    !projectGeneration ||
    !rootId ||
    !bindingId ||
    !runtimeEpoch ||
    !isOneOf(value.account, [
      "managedChatgpt",
      "providerManaged",
      "signedOut",
    ] as const) ||
    value.models.length > 128
  )
    return null;
  const ids = new Set<string>();
  let defaults = 0;
  const models: CodexDiscoveryModel[] = [];
  for (const candidate of value.models) {
    if (!isRecord(candidate)) return null;
    const id = safeIdentifier(candidate.id);
    const name = safeIdentifier(candidate.name);
    if (!id || !name || typeof candidate.isDefault !== "boolean" || ids.has(id))
      return null;
    ids.add(id);
    defaults += Number(candidate.isDefault);
    models.push({
      id: id as CodexDiscoveryModel["id"],
      name,
      isDefault: candidate.isDefault,
    });
  }
  const defaultModelId =
    value.defaultModelId === null ? null : safeIdentifier(value.defaultModelId);
  if (
    (value.account === "signedOut" &&
      (models.length !== 0 || defaultModelId !== null)) ||
    (value.account === "managedChatgpt" && models.length === 0) ||
    defaults > 1 ||
    (value.defaultModelId !== null && !defaultModelId) ||
    (defaultModelId !== null && !ids.has(defaultModelId)) ||
    (defaults === 0 && defaultModelId !== null) ||
    (defaults === 1 &&
      !models.some((model) => model.id === defaultModelId && model.isDefault))
  )
    return null;
  return {
    projectId: projectId as CodexDiscoveryResponse["projectId"],
    projectGeneration:
      projectGeneration as CodexDiscoveryResponse["projectGeneration"],
    rootId,
    bindingId: bindingId as CodexDiscoveryResponse["bindingId"],
    runtimeEpoch: runtimeEpoch as CodexDiscoveryResponse["runtimeEpoch"],
    account: value.account,
    models,
    defaultModelId: defaultModelId as CodexDiscoveryResponse["defaultModelId"],
  };
}

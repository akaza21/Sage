import type {
  AcpChatPage,
  AcpItem,
  AcpMutationResult,
  AgentSessionProjection,
  CodexDiscoveryResponse,
  HarnessActivityProjection,
  TurnProjection,
} from "../domain/acp";

const ACP_PAGE_LIMITS = {
  sessions: 32,
  turns: 20,
  items: 256,
  itemsPerTurn: 512,
  activities: 128,
} as const;

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

export function safeIdentifier(value: unknown): string | null {
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    value.length > 512 ||
    value.includes("\0")
  ) {
    return null;
  }
  return new TextEncoder().encode(value).byteLength <= 512 ? value : null;
}

export function safeCounter(value: unknown, allowZero: boolean): string | null {
  if (typeof value !== "string" || !/^(0|[1-9][0-9]{0,18})$/.test(value)) {
    return null;
  }
  if (value.length === 19 && value > "9223372036854775807") {
    return null;
  }
  return allowZero || value !== "0" ? value : null;
}

export function isOneOf<const Value extends string>(
  value: unknown,
  allowed: readonly Value[],
): value is Value {
  return typeof value === "string" && allowed.includes(value as Value);
}

function safeText(value: unknown, maximumBytes: number): string | null {
  if (typeof value !== "string") return null;
  return new TextEncoder().encode(value).byteLength <= maximumBytes
    ? value
    : null;
}

function safeOrdinal(value: unknown): number | null {
  return Number.isSafeInteger(value) && value >= 0 ? value : null;
}

function safeTimestamp(value: unknown): number | null {
  return Number.isSafeInteger(value) ? value : null;
}

function safeNullableIdentifier(value: unknown): string | null | undefined {
  return value === null ? null : (safeIdentifier(value) ?? undefined);
}

function safeNullableText(
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
  if (
    value.schemaVersion !== 1 ||
    !protocolVersion ||
    extensionSchemaId === undefined ||
    flags.some((flag) => typeof flag !== "boolean") ||
    !Number.isSafeInteger(value.modelCount) ||
    value.modelCount < 0 ||
    value.modelCount > 128 ||
    !Number.isSafeInteger(value.configCount) ||
    value.configCount < 0 ||
    value.configCount > 128
  ) {
    return null;
  }
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
    modelCount: value.modelCount,
    configCount: value.configCount,
  } as AgentSessionProjection["capabilities"];
}

function safeSession(value: unknown): AgentSessionProjection | null {
  if (!isRecord(value)) return null;
  const id = safeIdentifier(value.id);
  const bindingId = safeIdentifier(value.bindingId);
  const modelId = safeNullableIdentifier(value.modelId);
  const runtimeEpoch = safeNullableCounter(value.runtimeEpoch, false);
  const nextRuntimeEpoch = safeNullableCounter(value.nextRuntimeEpoch, false);
  const revision = safeCounter(value.revision, false);
  const projectionRevision = safeCounter(value.projectionRevision, false);
  const capabilities = safeCapabilities(value.capabilities);
  if (
    !id ||
    safeOrdinal(value.ordinal) === null ||
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
    !revision ||
    !projectionRevision ||
    typeof value.legacy !== "boolean"
  ) {
    return null;
  }
  return {
    id,
    ordinal: value.ordinal,
    bindingId,
    modelId,
    state: value.state,
    residency: value.residency,
    runtimeEpoch,
    nextRuntimeEpoch,
    capabilities,
    revision,
    projectionRevision,
    legacy: value.legacy,
  } as AgentSessionProjection;
}

function safeNullableCounter(
  value: unknown,
  allowZero: boolean,
): string | null | undefined {
  return value === null ? null : (safeCounter(value, allowZero) ?? undefined);
}

function safeItem(value: unknown): AcpItem | null {
  if (!isRecord(value) || !isRecord(value.payload)) return null;
  const ids = [
    value.id,
    value.projectId,
    value.chatId,
    value.sessionId,
    value.turnId,
  ].map(safeIdentifier);
  const activityId = safeNullableIdentifier(value.activityId);
  const runtimeEpoch = safeCounter(value.runtimeEpoch, false);
  const revision = safeCounter(value.revision, false);
  if (
    ids.some((id) => !id) ||
    !runtimeEpoch ||
    safeOrdinal(value.ordinal) === null ||
    activityId === undefined ||
    !isOneOf(value.origin, ["primary", "harnessActivity"] as const) ||
    !isOneOf(value.lifecycle, [
      "started",
      "streaming",
      "completed",
      "failed",
      "cancelled",
      "unsupported",
    ] as const) ||
    !revision ||
    safeTimestamp(value.createdAt) === null ||
    safeTimestamp(value.updatedAt) === null ||
    (value.terminalAt !== null && safeTimestamp(value.terminalAt) === null) ||
    !isOneOf(value.kind, [
      "text",
      "thoughtSummary",
      "plan",
      "toolCall",
      "toolUpdateResult",
      "fileChangeDiff",
      "usage",
      "permissionRequest",
      "error",
      "unsupported",
      "collaboration",
    ] as const) ||
    (value.toolState !== null &&
      !isOneOf(value.toolState, [
        "pending",
        "inProgress",
        "waitingForApproval",
        "completed",
        "failed",
        "declined",
        "cancelled",
      ] as const))
  ) {
    return null;
  }
  return {
    id: ids[0],
    projectId: ids[1],
    chatId: ids[2],
    sessionId: ids[3],
    turnId: ids[4],
    runtimeEpoch,
    ordinal: value.ordinal,
    origin: value.origin,
    activityId,
    lifecycle: value.lifecycle,
    revision,
    createdAt: value.createdAt,
    updatedAt: value.updatedAt,
    terminalAt: value.terminalAt,
    toolState: value.toolState,
    kind: value.kind,
    payload: value.payload,
  } as unknown as AcpItem;
}

function safeActivity(value: unknown): HarnessActivityProjection | null {
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
    !Number.isSafeInteger(value.depth) ||
    value.depth < 0 ||
    value.depth > 255 ||
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
  ) {
    return null;
  }
  return {
    id,
    turnId,
    parentActivityId,
    parentItemId,
    role: value.role,
    lifecycle: value.lifecycle,
    depth: value.depth,
    provenance: {
      schemaVersion: 1,
      sourceKind: value.provenance.sourceKind,
      externalKind,
      extensionSchemaId,
      safeLabel,
    },
    revision,
  } as HarnessActivityProjection;
}

function safeTurn(value: unknown): TurnProjection | null {
  if (
    !isRecord(value) ||
    !Array.isArray(value.items) ||
    !Array.isArray(value.activities)
  ) {
    return null;
  }
  if (
    value.items.length > ACP_PAGE_LIMITS.itemsPerTurn ||
    value.activities.length > ACP_PAGE_LIMITS.activities
  ) {
    return null;
  }
  const id = safeIdentifier(value.id);
  const sessionId = safeIdentifier(value.sessionId);
  const runtimeEpoch = safeCounter(value.runtimeEpoch, false);
  const revision = safeCounter(value.revision, false);
  if (
    !id ||
    !sessionId ||
    !runtimeEpoch ||
    safeOrdinal(value.ordinal) === null ||
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
    safeTimestamp(value.createdAt) === null ||
    safeTimestamp(value.updatedAt) === null ||
    (value.terminalAt !== null && safeTimestamp(value.terminalAt) === null)
  ) {
    return null;
  }
  const items = value.items.map(safeItem);
  const activities = value.activities.map(safeActivity);
  if (items.some((item) => !item) || activities.some((activity) => !activity)) {
    return null;
  }
  return {
    id,
    sessionId,
    runtimeEpoch,
    ordinal: value.ordinal,
    state: value.state,
    revision,
    createdAt: value.createdAt,
    updatedAt: value.updatedAt,
    terminalAt: value.terminalAt,
    items: items as AcpItem[],
    activities: activities as HarnessActivityProjection[],
  } as TurnProjection;
}

export function safeAcpMutationResponse(
  value: unknown,
): AcpMutationResult | null {
  if (!isRecord(value) || !isRecord(value.projection)) return null;
  const projection = value.projection;
  const currentSessionId =
    projection.currentSessionId === null
      ? null
      : safeIdentifier(projection.currentSessionId);
  const title = safeText(projection.title, 8192);
  if (
    !safeIdentifier(projection.projectId) ||
    !safeIdentifier(projection.chatId) ||
    title === null ||
    !safeCounter(projection.projectionRevision, false) ||
    (projection.currentSessionId !== null && !currentSessionId) ||
    !Array.isArray(projection.sessions) ||
    !Array.isArray(projection.turns) ||
    projection.sessions.length > ACP_PAGE_LIMITS.sessions ||
    projection.turns.length > ACP_PAGE_LIMITS.turns ||
    (projection.nextCursor !== null && !safeIdentifier(projection.nextCursor))
  ) {
    return null;
  }
  const sessions = projection.sessions.map(safeSession);
  const turns = projection.turns.map(safeTurn);
  if (sessions.some((session) => !session) || turns.some((turn) => !turn)) {
    return null;
  }
  const safeSessions = sessions as AgentSessionProjection[];
  const safeTurns = turns as TurnProjection[];
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
    (currentSessionId !== null && !sessionIds.has(currentSessionId)) ||
    safeTurns.some(
      (turn) =>
        !sessionIds.has(turn.sessionId) ||
        turn.items.some(
          (item) =>
            item.projectId !== projection.projectId ||
            item.chatId !== projection.chatId ||
            item.sessionId !== turn.sessionId ||
            item.turnId !== turn.id,
        ) ||
        turn.activities.some((activity) => activity.turnId !== turn.id),
    )
  ) {
    return null;
  }
  return {
    projection: {
      projectId: safeIdentifier(
        projection.projectId,
      ) as AcpChatPage["projectId"],
      chatId: safeIdentifier(projection.chatId) as AcpChatPage["chatId"],
      title,
      projectionRevision: safeCounter(
        projection.projectionRevision,
        false,
      ) as AcpChatPage["projectionRevision"],
      currentSessionId: currentSessionId as AcpChatPage["currentSessionId"],
      sessions: safeSessions,
      turns: safeTurns,
      nextCursor: projection.nextCursor as AcpChatPage["nextCursor"],
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
    !isOneOf(value.account, ["managedChatgpt", "signedOut"] as const) ||
    value.models.length > 128
  ) {
    return null;
  }
  const ids = new Set<string>();
  let defaults = 0;
  const models = [];
  for (const candidate of value.models) {
    if (!isRecord(candidate)) return null;
    const id = safeIdentifier(candidate.id);
    const name = safeIdentifier(candidate.name);
    if (
      !id ||
      !name ||
      typeof candidate.isDefault !== "boolean" ||
      ids.has(id)
    ) {
      return null;
    }
    ids.add(id);
    defaults += Number(candidate.isDefault);
    models.push({ id, name, isDefault: candidate.isDefault });
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
  ) {
    return null;
  }
  return {
    projectId,
    projectGeneration,
    rootId,
    bindingId,
    runtimeEpoch,
    account: value.account,
    models,
    defaultModelId,
  } as unknown as CodexDiscoveryResponse;
}

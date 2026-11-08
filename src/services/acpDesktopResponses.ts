import {
  isOneOf,
  isRecord,
  safeCounter,
  safeIdentifier,
  safeOrdinal,
  safeNullableCounter,
  safeNullableIdentifier,
  safeText,
} from "./acpDesktopGuards";
import type {
  AcpChatPage,
  AcpMutationResult,
  AgentSessionProjection,
} from "../domain/acp";
import type { AcpGoalState } from "../domain/acpGoal";
import { safeTurn } from "./acpDesktopResponseTurns";

const ACP_PAGE_LIMITS = {
  sessions: 32,
  turns: 20,
  items: 256,
  activities: 128,
} as const;

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
    ...(value.promptResources === true ? { promptResources: true } : {}),
    ...(value.promptImages === true ? { promptImages: true } : {}),
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

import {
  decodeRestoredSessionConfig,
  decodeSessionConfig,
  decodeSavedChoiceNotice,
} from "../domain/acpSessionConfig";

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
  decodeGoal?: (value: unknown) => AcpGoalState | null,
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
  const forkedFrom =
    projection.forkedFrom === undefined
      ? undefined
      : safeIdentifier(projection.forkedFrom);
  // Display-only extras: an unsafe value is dropped, never trusted.
  const forkedFromTurnId = safeIdentifier(projection.forkedFromTurnId);
  const epoch = projection.childProjectionEpoch as number;
  if (
    forkedFrom === null ||
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
  // Restored config belongs to the current session and never sits beside a
  // live one; anything else is dropped rather than shown.
  const restoredConfig =
    projection.restoredSessionConfig == null || sessionConfig
      ? null
      : decodeRestoredSessionConfig(projection.restoredSessionConfig);
  const restoredSessionConfig =
    restoredConfig?.sageAgentSessionId === currentSessionId
      ? restoredConfig
      : null;
  const sessionConfigNotice =
    projection.sessionConfigNotice == null
      ? null
      : decodeSavedChoiceNotice(projection.sessionConfigNotice);
  const goal =
    projection.goal === undefined || !decodeGoal
      ? undefined
      : (decodeGoal(projection.goal) ?? { kind: "unavailable" as const });
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
      ...(restoredSessionConfig ? { restoredSessionConfig } : {}),
      ...(sessionConfigNotice ? { sessionConfigNotice } : {}),
      ...(goal ? { goal } : {}),
      turns: safeTurns,
      nextCursor: nextCursor as AcpChatPage["nextCursor"],
      ...(forkedFrom
        ? { forkedFrom: forkedFrom as AcpChatPage["chatId"] }
        : {}),
      ...(forkedFromTurnId && {
        forkedFromTurnId: forkedFromTurnId as AcpChatPage["forkedFromTurnId"],
      }),
      ...(Number.isSafeInteger(epoch) &&
        epoch > 0 && { childProjectionEpoch: epoch }),
    },
  };
}

import type {
  AcpCapabilitySet,
  AgentSessionProjection,
  EntityRevision,
  ProjectionRevision,
  SageAgentSessionId,
} from "./acp";

export function capabilitySetFixture(
  overrides: Partial<AcpCapabilitySet> = {},
): AcpCapabilitySet {
  return {
    schemaVersion: 1,
    protocolVersion: "fixture-v1",
    newSession: true,
    resumeSession: false,
    loadSession: false,
    closeSession: true,
    prompt: true,
    cancel: true,
    nativeSessionFork: false,
    activeTurnSteer: false,
    childConversationProjection: false,
    childTranscriptRead: false,
    collaborationEventProjection: false,
    multiRootExtensionSchemaId: null,
    modelCount: 1,
    configCount: 0,
    ...overrides,
  };
}

export function agentSessionFixture(
  overrides: Partial<AgentSessionProjection> = {},
): AgentSessionProjection {
  return {
    id: "session-a" as SageAgentSessionId,
    ordinal: 0,
    bindingId: "binding-a" as AgentSessionProjection["bindingId"],
    modelId: null,
    state: "ready",
    residency: "cold",
    runtimeEpoch: null,
    nextRuntimeEpoch: null,
    capabilities: capabilitySetFixture(),
    permissionRequests: false,
    revision: "1" as EntityRevision,
    projectionRevision: "1" as ProjectionRevision,
    legacy: false,
    ...overrides,
  };
}

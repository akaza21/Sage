import type {
  AcpCapabilitySet,
  AgentSessionProjection,
  EntityRevision,
  ProjectGeneration,
  RuntimeEpoch,
  SageAgentSessionId,
  SageChatId,
  SageItemId,
  SageProjectId,
  SageTurnId,
} from "../../../src/domain/acp";

export const FIXED_NOW = Date.UTC(2026, 7, 31, 9, 30, 0);
export const PROJECT_ID = "visual-project" as SageProjectId;
export const ROOT_ID = "visual-root";
export const GENERATION = "0" as ProjectGeneration;
export const RUNTIME_EPOCH = "7" as RuntimeEpoch;
export const SESSION_ID = "visual-session" as SageAgentSessionId;
export const BINDING_ID = "visual-binding";
export const MODEL_ID = "visual-model";

const capabilities: AcpCapabilitySet = {
  schemaVersion: 1,
  protocolVersion: "1.0",
  newSession: true,
  resumeSession: true,
  loadSession: true,
  closeSession: true,
  prompt: true,
  cancel: true,
  nativeSessionFork: false,
  activeTurnSteer: false,
  childConversationProjection: true,
  childTranscriptRead: true,
  collaborationEventProjection: true,
  multiRootExtensionSchemaId: null,
  modelCount: 1,
  configCount: 1,
};

export function session(state: AgentSessionProjection["state"] = "ready") {
  return {
    id: SESSION_ID,
    ordinal: 1,
    bindingId: BINDING_ID,
    modelId: MODEL_ID,
    state,
    residency: "hot",
    runtimeEpoch: RUNTIME_EPOCH,
    nextRuntimeEpoch: null,
    capabilities,
    revision: "1",
    projectionRevision: "1",
    legacy: false,
  } as AgentSessionProjection;
}

export interface ItemInput {
  chatId: SageChatId;
  turnId: SageTurnId;
  ordinal: number;
  createdAt: number;
}

export function itemIdentity(input: ItemInput) {
  const id = `${input.turnId}-item-${input.ordinal}` as SageItemId;
  return {
    id,
    projectId: PROJECT_ID,
    chatId: input.chatId,
    sessionId: SESSION_ID,
    turnId: input.turnId,
    runtimeEpoch: RUNTIME_EPOCH,
    ordinal: input.ordinal,
    origin: "primary" as const,
    activityId: null,
    lifecycle: "completed" as const,
    revision: "1" as EntityRevision,
    createdAt: input.createdAt,
    updatedAt: input.createdAt + 1_000,
    terminalAt: input.createdAt + 1_000,
  };
}

import type { ResourceRef } from "./workbench";

type Brand<Value, Name extends string> = Value & {
  readonly __sageBrand: Name;
};

export type SageProjectId = Brand<string, "SageProjectId">;
export type SageChatId = Brand<string, "SageChatId">;
export type SageAgentSessionId = Brand<string, "SageAgentSessionId">;
export type SageTurnId = Brand<string, "SageTurnId">;
export type SageItemId = Brand<string, "SageItemId">;
export type SageActivityId = Brand<string, "SageActivityId">;
export type SagePermissionRequestId = Brand<string, "SagePermissionRequestId">;
export type ClientIntentId = Brand<string, "ClientIntentId">;
export type CuratedBindingId = Brand<string, "CuratedBindingId">;
export type CuratedModelId = Brand<string, "CuratedModelId">;
export type AcpPageCursor = Brand<string, "AcpPageCursor">;
export type ProjectionRevision = Brand<string, "ProjectionRevision">;
export type ProjectGeneration = Brand<string, "ProjectGeneration">;
export type RuntimeEpoch = Brand<string, "RuntimeEpoch">;
export type EntityRevision = Brand<string, "EntityRevision">;

export type AgentSessionState =
  | "created"
  | "starting"
  | "ready"
  | "active"
  | "recovering"
  | "closing"
  | "closed"
  | "failed"
  | "incompatible";

export type SessionResidency = "hot" | "cold";

export type TurnState =
  | "queued"
  | "sending"
  | "running"
  | "waitingModel"
  | "waitingTool"
  | "waitingApproval"
  | "waitingUser"
  | "cancelling"
  | "completed"
  | "interrupted"
  | "cancelled"
  | "failed"
  | "deliveryUnknown";

export type ItemLifecycle =
  | "started"
  | "streaming"
  | "completed"
  | "failed"
  | "cancelled"
  | "unsupported";

export type ToolState =
  | "pending"
  | "inProgress"
  | "waitingForApproval"
  | "completed"
  | "failed"
  | "declined"
  | "cancelled";

export type PermissionState =
  | "received"
  | "declinePending"
  | "declineCommitted"
  | "responseQueued"
  | "resolvedDeclined";

export type ActivityState =
  "observed" | "active" | "completed" | "failed" | "cancelled" | "unsupported";

export interface AcpCapabilitySet {
  schemaVersion: 1;
  protocolVersion: string;
  newSession: boolean;
  resumeSession: boolean;
  loadSession: boolean;
  closeSession: boolean;
  prompt: boolean;
  cancel: boolean;
  nativeSessionFork: boolean;
  activeTurnSteer: boolean;
  childConversationProjection: boolean;
  childTranscriptRead: boolean;
  collaborationEventProjection: boolean;
  multiRootExtensionSchemaId: string | null;
  modelCount: number;
  configCount: number;
}

export type SafeResourceLocation =
  | { kind: "resource"; resource: ResourceRef }
  | {
      kind: "unavailable";
      reason:
        | "ambiguousRoot"
        | "foreign"
        | "missing"
        | "noncanonical"
        | "unsupported";
      label: string;
    };

export type ToolPresentation =
  | { kind: "generic"; label: string; summary: string | null }
  | {
      kind: "file" | "diff";
      label: string;
      resources: readonly SafeResourceLocation[];
    }
  | { kind: "command"; label: string; summary: string | null }
  | {
      kind: "search";
      label: string;
      querySummary: string;
      resources: readonly ResourceRef[];
    }
  | { kind: "web"; label: string; summary: string | null }
  | {
      kind: "mcp";
      label: string;
      serverLabel: string;
      toolLabel: string;
    }
  | {
      kind: "subagent";
      label: string;
      activityId: SageActivityId | null;
    };

export interface FileChangeProjection {
  resource: SafeResourceLocation;
  operation: "created" | "updated" | "deleted" | "renamed" | "unknown";
  originalResource: SafeResourceLocation | null;
  summary: string;
}

export interface AcpItemIdentity {
  id: SageItemId;
  projectId: SageProjectId;
  chatId: SageChatId;
  sessionId: SageAgentSessionId;
  turnId: SageTurnId;
  runtimeEpoch: RuntimeEpoch;
  ordinal: number;
  origin: "primary" | "harnessActivity";
  activityId: SageActivityId | null;
  lifecycle: ItemLifecycle;
  revision: EntityRevision;
  createdAt: number;
  updatedAt: number;
  terminalAt: number | null;
}

type PlainItem<Kind extends string, Payload> = AcpItemIdentity & {
  kind: Kind;
  toolState: null;
  payload: Payload;
};

type ToolItem<Kind extends string, Payload> = AcpItemIdentity & {
  kind: Kind;
  toolState: ToolState;
  payload: Payload;
};

export type AcpItem =
  | PlainItem<"text", { role: "user" | "assistant" | "system"; text: string }>
  | PlainItem<"thoughtSummary", { summary: string }>
  | PlainItem<
      "plan",
      {
        entries: readonly {
          label: string;
          state: "pending" | "inProgress" | "completed" | "failed";
        }[];
      }
    >
  | ToolItem<
      "toolCall",
      {
        presentation: ToolPresentation;
        inputSummary: string | null;
      }
    >
  | ToolItem<
      "toolUpdateResult",
      {
        callItemId: SageItemId | null;
        presentation: ToolPresentation;
        outputSummary: string | null;
      }
    >
  | PlainItem<
      "fileChangeDiff",
      { changes: readonly FileChangeProjection[]; truncated: boolean }
    >
  | PlainItem<
      "usage",
      {
        inputTokens: number | null;
        outputTokens: number | null;
        cachedInputTokens: number | null;
        totalTokens: number | null;
      }
    >
  | ToolItem<
      "permissionRequest",
      {
        requestId: SagePermissionRequestId;
        state: PermissionState;
        label: string;
        summary: string;
      }
    >
  | PlainItem<"error", { code: string; message: string; recoverable: boolean }>
  | PlainItem<
      "unsupported",
      { category: string; summary: string; truncated: boolean }
    >
  | PlainItem<
      "collaboration",
      {
        activityId: SageActivityId;
        label: string;
        summary: string;
        receiverLabels: readonly string[];
      }
    >;

export interface HarnessActivityProjection {
  id: SageActivityId;
  turnId: SageTurnId;
  parentActivityId: SageActivityId | null;
  parentItemId: SageItemId;
  role: "root" | "fork" | "child" | "unknown";
  lifecycle: ActivityState;
  depth: number;
  provenance: {
    schemaVersion: 1;
    sourceKind: "tool" | "extension" | "collaboration" | "unknown";
    externalKind: string;
    extensionSchemaId: string | null;
    safeLabel: string | null;
  };
  revision: EntityRevision;
}

export interface TurnProjection {
  id: SageTurnId;
  sessionId: SageAgentSessionId;
  runtimeEpoch: RuntimeEpoch;
  ordinal: number;
  state: TurnState;
  revision: EntityRevision;
  createdAt: number;
  updatedAt: number;
  terminalAt: number | null;
  items: readonly AcpItem[];
  activities: readonly HarnessActivityProjection[];
}

export interface AgentSessionProjection {
  id: SageAgentSessionId;
  ordinal: number;
  bindingId: CuratedBindingId;
  modelId: CuratedModelId | null;
  state: AgentSessionState;
  residency: SessionResidency;
  runtimeEpoch: RuntimeEpoch | null;
  nextRuntimeEpoch: RuntimeEpoch | null;
  capabilities: AcpCapabilitySet;
  revision: EntityRevision;
  projectionRevision: ProjectionRevision;
  legacy: boolean;
}

export interface AcpChatPage {
  projectId: SageProjectId;
  chatId: SageChatId;
  title: string;
  projectionRevision: ProjectionRevision;
  currentSessionId: SageAgentSessionId | null;
  sessions: readonly AgentSessionProjection[];
  turns: readonly TurnProjection[];
  nextCursor: AcpPageCursor | null;
}

export interface GetAcpChatPageRequest {
  projectId: SageProjectId;
  projectGeneration: ProjectGeneration;
  chatId: SageChatId;
  cursor: AcpPageCursor | null;
}

export interface GetCodexDiscoveryRequest {
  projectId: SageProjectId;
  projectGeneration: ProjectGeneration;
  bindingId: CuratedBindingId;
}

export interface CodexDiscoveryModel {
  id: CuratedModelId;
  name: string;
  isDefault: boolean;
}

export interface CodexDiscoveryResponse {
  projectId: SageProjectId;
  projectGeneration: ProjectGeneration;
  rootId: string;
  bindingId: CuratedBindingId;
  runtimeEpoch: RuntimeEpoch;
  account: "managedChatgpt" | "signedOut";
  models: readonly CodexDiscoveryModel[];
  defaultModelId: CuratedModelId | null;
}

export interface CodexAuthScope {
  projectId: SageProjectId;
  projectGeneration: ProjectGeneration;
  bindingId: CuratedBindingId;
}

export type CodexAuthPhase = "signedOut" | "waiting" | "recovery" | "signedIn";

export interface CodexAuthResponse extends CodexAuthScope {
  phase: CodexAuthPhase;
  revision: string;
}

export interface AcpMutationIntent {
  projectId: SageProjectId;
  projectGeneration: ProjectGeneration;
  chatId: SageChatId;
  expectedProjectionRevision: ProjectionRevision;
  clientIntentId: ClientIntentId;
}

export interface AcpSessionMutationIntent extends AcpMutationIntent {
  sessionId: SageAgentSessionId;
  runtimeEpoch: RuntimeEpoch;
}

export interface StartAcpSessionRequest extends AcpMutationIntent {
  bindingId: CuratedBindingId;
  modelId: CuratedModelId | null;
}

export type SendAcpTurnRequest =
  | (AcpSessionMutationIntent & { target: "session"; prompt: string })
  | (AcpMutationIntent & {
      target: "newSession";
      bindingId: CuratedBindingId;
      modelId: CuratedModelId | null;
      prompt: string;
    });

export interface CancelAcpTurnRequest extends AcpSessionMutationIntent {
  turnId: SageTurnId;
  expectedTurnState: TurnState;
  expectedTurnRevision: EntityRevision;
}

export type ActivateAcpSessionRequest = AcpSessionMutationIntent;
export type CloseAcpSessionRequest = AcpSessionMutationIntent;

export interface AcpMutationResult {
  projection: AcpChatPage;
}

export interface AcpProjectionChangedEvent {
  projectId: SageProjectId;
  chatId: SageChatId;
  sessionId: SageAgentSessionId;
  projectionRevision: ProjectionRevision;
}

export type AcpCommandFailure =
  | {
      kind: "unavailable";
      reason:
        | "desktopRuntimeUnavailable"
        | "bindingUnavailable"
        | "capabilityUnavailable"
        | "platformUnsupported"
        | "sessionUnavailable";
      message: string;
    }
  | {
      kind: "conflict";
      resource:
        | "projectGeneration"
        | "projectionRevision"
        | "session"
        | "runtimeEpoch"
        | "turnState"
        | "turnRevision"
        | "clientIntent";
      message: string;
    }
  | {
      kind: "capacity";
      resource:
        | "activeTurn"
        | "liveTree"
        | "history"
        | "pageRead"
        | "storage"
        | "queue";
      limit: number;
      message: string;
    }
  | { kind: "invalidRequest"; message: string }
  | { kind: "recoveryRequired"; message: string }
  | { kind: "internal"; message: string };

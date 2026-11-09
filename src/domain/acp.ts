export type {
  AcpChatPage,
  AcpItem,
  AgentEditDisposition,
  AgentEditDispositionState,
  AgentSessionProjection,
  FileChangeProjection,
  HarnessActivityProjection,
  SafeResourceLocation,
  ToolPresentation,
  TurnProjection,
  TurnSessionActivity,
} from "./acpProjection";
import type { AcpChatPage } from "./acpProjection";
import type { AcpSessionAction } from "./acpSessionAction";
import type { AttachedContext } from "./attachedContext";
export type { AcpReviewTarget, AcpSessionAction } from "./acpSessionAction";

export type {
  AnswerUserInputRequest,
  AnswerUserInputResponse,
  PermissionDecision,
  PermissionState,
  ResolvePermissionDecisionRequest,
  ResolvePermissionDecisionResponse,
  UserInputAnswer,
  UserInputQuestion,
  UserInputRequest,
} from "./acpPermission";

type Brand<Value, Name extends string> = Value & {
  readonly __sageBrand: Name;
};

export type SageProjectId = Brand<string, "SageProjectId">;
export type SageRootId = Brand<string, "SageRootId">;
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
export type ConfigRevision = Brand<string, "ConfigRevision">;
import type { AcpConfigValue } from "./acpSessionConfig";
export type {
  AcpSessionConfig,
  AcpSessionConfigChangedV1,
  AcpConfigOption,
  AcpConfigValue,
} from "./acpSessionConfig";
export interface SetAcpSessionModeRequest extends AcpSessionMutationIntent {
  expectedConfigRevision: string;
  modeId: string;
}
export interface SetAcpSessionConfigOptionRequest extends AcpSessionMutationIntent {
  expectedConfigRevision: string;
  configId: string;
  value: AcpConfigValue;
}
/**
 * A choice for a session with no live runtime: stored, shown as restored,
 * and applied before the session's next prompt.
 */
export interface QueueAcpSessionConfigChoiceRequest {
  projectId: SageProjectId;
  projectGeneration: ProjectGeneration;
  chatId: SageChatId;
  expectedProjectionRevision: ProjectionRevision;
  sessionId: SageAgentSessionId;
  choice: { modeId: string } | { configId: string; value: AcpConfigValue };
}
export type EntityRevision = Brand<string, "EntityRevision">;

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

export type TurnTerminalReason =
  | "authRequired"
  | "harnessExited"
  | "protocolError"
  | "cancelledByUser"
  | "refusedVersion"
  | "timeout"
  | "other";

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
  /** Absent in older sessions; native admission treats absence as unsupported. */
  promptResources?: boolean;
  /** Standard ACP `promptCapabilities.image`; absent means unsupported. */
  promptImages?: boolean;
  cancel: boolean;
  nativeSessionFork: boolean;
  /** Absent unless the harness negotiated it. */
  nativeSessionCompact?: boolean;
  /** Absent unless the harness negotiated it. */
  nativeReview?: boolean;
  activeTurnSteer: boolean;
  childConversationProjection: boolean;
  childTranscriptRead: boolean;
  collaborationEventProjection: boolean;
  multiRootExtensionSchemaId: string | null;
  modelCount: number;
  configCount: number;
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

export type CodexAccount =
  "managedChatgpt" | "apiKey" | "providerManaged" | "signedOut";

export interface CodexDiscoveryResponse {
  projectId: SageProjectId;
  projectGeneration: ProjectGeneration;
  rootId: string;
  bindingId: CuratedBindingId;
  runtimeEpoch: RuntimeEpoch;
  account: CodexAccount;
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

export interface RunAcpSessionActionRequest extends AcpSessionMutationIntent {
  action: AcpSessionAction;
}

export interface StartAcpSessionRequest extends AcpMutationIntent {
  bindingId: CuratedBindingId;
  modelId: CuratedModelId | null;
}

interface AcpTurnContent {
  prompt: string;
  attachedContext?: readonly AttachedContext[];
}

/**
 * The staged prompt files a send carries: the chat's batch at a revision, or
 * for Retry of a turn the files that turn's prompt carried. Native refuses a
 * Retry whose files are no longer held rather than sending it without them.
 */
export type StagedSelection = { revision: string } | { retryOf: SageTurnId };

export type SendAcpTurnRequest =
  | (AcpSessionMutationIntent &
      AcpTurnContent & { target: "session"; staged?: StagedSelection })
  | (AcpMutationIntent &
      AcpTurnContent & {
        target: "newSession";
        bindingId: CuratedBindingId;
        modelId: CuratedModelId | null;
      });

export interface CancelAcpTurnRequest extends AcpSessionMutationIntent {
  turnId: SageTurnId;
  expectedTurnState: TurnState;
  expectedTurnRevision: EntityRevision;
}

export interface SteerAcpTurnRequest extends AcpSessionMutationIntent {
  turnId: SageTurnId;
  expectedTurnRevision: EntityRevision;
  prompt: string;
  attachedContext?: readonly AttachedContext[];
}

export type ActivateAcpSessionRequest = AcpSessionMutationIntent;
export type CloseAcpSessionRequest = AcpSessionMutationIntent;
export type EndAcpSessionRequest = AcpSessionMutationIntent;

export interface AcpMutationResult {
  projection: AcpChatPage;
}

export interface AcpProjectionChangedEvent {
  projectId: SageProjectId;
  chatId: SageChatId;
  sessionId: SageAgentSessionId;
  projectionRevision: ProjectionRevision;
}

type AcpCommandFailureDetail =
  | {
      kind: "unavailable";
      reason:
        | "desktopRuntimeUnavailable"
        | "bindingUnavailable"
        | "capabilityUnavailable"
        | "platformUnsupported"
        | "sessionUnavailable"
        | "imageInputUnsupported";
      message: string;
    }
  | {
      kind: "conflict";
      resource:
        | "projectGeneration"
        | "configRevision"
        | "projectionRevision"
        | "session"
        | "runtimeEpoch"
        | "turnState"
        | "turnRevision"
        | "clientIntent"
        | "steerOutcomeUnknown";
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
  | { kind: "signInRequired"; message: string }
  | { kind: "rejected"; message: string }
  | { kind: "recoveryRequired"; message: string }
  | { kind: "internal"; message: string };

export type AcpCommandFailure = AcpCommandFailureDetail & {
  promptDispatch?: "notStarted" | "unknown";
};

/** A rail read contains no transcript items, activities or session pages. */
export interface AcpChatStatus {
  projectId: SageProjectId;
  chatId: SageChatId;
  title: string;
  latestTurn: {
    state: TurnState;
    updatedAt: number;
    createdAt: number;
    terminalAt: number | null;
  } | null;
}

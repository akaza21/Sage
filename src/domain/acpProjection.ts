import type { AttachedContext } from "./attachedContext";
import type { AcpSessionAction } from "./acpSessionAction";
import type { AcpTurnCommand } from "./acpCommand";
import type { AcpImagePayload } from "./acpImage";
import type { AcpGoalState } from "./acpGoal";
import type {
  AcpRestoredSessionConfig,
  AcpSessionConfig,
  AcpSessionConfigNotice,
} from "./acpSessionConfig";
import type { BrowserOperationProjection } from "./browserOperation";
import type { FilePairMetadata } from "./agentDiff";
import type { UserInputRequest } from "./acpPermission";
import type {
  ActivityState,
  AgentSessionState,
  EntityRevision,
  ProjectionRevision,
  RuntimeEpoch,
  SageActivityId,
  SageAgentSessionId,
  SageChatId,
  SageItemId,
  SagePermissionRequestId,
  SageProjectId,
  SageTurnId,
  ToolState,
  TurnState,
  TurnTerminalReason,
  CuratedBindingId,
  CuratedModelId,
  SessionResidency,
  AcpCapabilitySet,
  AcpPageCursor,
  AcpItemIdentity,
} from "./acp";
import type { PermissionState } from "./acpPermission";
import type { ResourceRef } from "./workbench";

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
      displayResource?: ResourceRef | null;
    };

/** Byte cap of a running tool's status line; native `TOOL_PROGRESS_BYTES`. */
export const TOOL_PROGRESS_BYTES = 512;

export type ToolPresentation =
  | {
      kind: "read" | "edit" | "delete" | "imageView";
      label: string;
      resources: readonly SafeResourceLocation[];
    }
  /** The query, when the harness carried one, is the item's inputSummary. */
  | { kind: "webSearch"; label: string }
  | {
      kind: "move";
      label: string;
      resources: readonly SafeResourceLocation[];
      originalResources: readonly SafeResourceLocation[];
    }
  | {
      kind: "search";
      label: string;
      querySummary: string;
      resources: readonly SafeResourceLocation[];
      matchCount: number | null;
      truncated: boolean;
    }
  | {
      kind: "execute";
      label: string;
      summary: string | null;
      cwd: SafeResourceLocation | null;
      exitCode: number | null;
      outputTail: string | null;
      outputTruncated: boolean;
      resources: readonly SafeResourceLocation[];
      activity: "ordinary" | "declaredTest";
    }
  | {
      kind: "fetch";
      label: string;
      url: string | null;
      summary: string | null;
    }
  | { kind: "think"; label: string; summary: string | null }
  | {
      kind: "switchMode";
      label: string;
      modeId: string | null;
      modeLabel: string | null;
    }
  | {
      kind: "mcp";
      label: string;
      serverLabel: string;
      toolLabel: string;
      argumentsSummary: string | null;
      resultSummary: string | null;
      /** Latest status line of a running call; absent once it is terminal. */
      progress?: string;
    }
  | {
      kind: "subagent";
      label: string;
      activityId: SageActivityId | null;
      taskLifecycle?: boolean;
    }
  | {
      kind: "other";
      label: string;
      rawKind: string | null;
      summary: string | null;
    };

export type FileChangeProjection = FilePairMetadata & {
  resource: SafeResourceLocation;
  operation: "created" | "updated" | "deleted" | "renamed" | "unknown";
  originalResource: SafeResourceLocation | null;
  summary: string;
  additions: number | null;
  deletions: number | null;
};

export type AgentEditDispositionState = "accepted" | "rejected";

export interface AgentEditDisposition {
  projectId: SageProjectId;
  chatId: SageChatId;
  sessionId: SageAgentSessionId;
  turnId: SageTurnId;
  expectedRevision: EntityRevision;
  state: AgentEditDispositionState;
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
  /** A message the user steered into the running Turn, at its place. */
  | PlainItem<"userPrompt", { text: string }>
  | PlainItem<"thoughtSummary", { summary: string }>
  | PlainItem<"compactionNotice", Record<string, never>>
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
        outputSummary?: string | null;
      }
    >
  | PlainItem<
      "fileChangeDiff",
      { changes: readonly FileChangeProjection[]; truncated: boolean }
    >
  | PlainItem<"image", AcpImagePayload>
  | ToolItem<
      "permissionRequest",
      {
        requestId: SagePermissionRequestId;
        state: PermissionState;
        label: string;
        summary: string;
        /** `browserDeveloper` is a developer-mode card (ADR-0009b). */
        source?: "harness" | "browser" | "browserDeveloper";
        offeredOptionKinds?: Array<
          "allow_once" | "allow_always" | "reject_once" | "reject_always"
        > | null;
        /** The kind of tool the request gates, when the projection names it. */
        category?: ToolPresentation["kind"];
        /** The harness's own explanation; absent when it gave none. */
        reason?: string | null;
        /** The harness's questions when it asks the user, not for approval. */
        userInput?: UserInputRequest | null;
      }
    >
  | PlainItem<
      "error",
      { kind?: "attempt"; code: string; message: string; recoverable: boolean }
    >
  | PlainItem<
      "unsupported",
      { category: string; summary: string; truncated: boolean }
    >
  | PlainItem<
      "collaboration",
      {
        activityId: SageActivityId | null;
        label: string;
        summary: string;
        receiverLabels: readonly string[];
        operation?: string | null;
        agentKey?: string | null;
        agentKeys?: readonly string[];
        agentStates?: readonly (readonly [string, string])[];
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

/**
 * A session activity row (a hook run, say) in the work list of the Turn
 * that owns it; native `TurnSessionActivityProjection`. A Turn
 * holds at most `SESSION_ACTIVITIES_MAX` of them.
 */
export interface TurnSessionActivity {
  kind: string;
  label: string;
  state: "running" | "completed" | "failed" | "cancelled";
  progress?: string;
}

export const SESSION_ACTIVITIES_MAX = 16;

export interface TurnProjection {
  attachedContext?: readonly AttachedContext[];
  /** The session action this Turn ran. */
  sessionAction?: AcpSessionAction;
  /** The Turn ran a session action this build cannot run again. */
  unrecognizedAction?: true;
  /** The listed command this Turn's prompt ran. */
  command?: AcpTurnCommand;
  id: SageTurnId;
  sessionId: SageAgentSessionId;
  runtimeEpoch: RuntimeEpoch;
  ordinal: number;
  state: TurnState;
  revision: EntityRevision;
  createdAt: number;
  updatedAt: number;
  terminalAt: number | null;
  usage?: {
    inputTokens: number;
    outputTokens: number;
    totalTokens: number;
    contextWindow?: number;
  };
  terminalReason?: {
    kind: TurnTerminalReason;
    message?: string;
    remediation?: string;
  };
  items: readonly AcpItem[];
  activities: readonly HarnessActivityProjection[];
  browserOperations: readonly BrowserOperationProjection[];
  browserOperationsCollapsed: number;
  sessionActivities?: readonly TurnSessionActivity[];
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
  permissionRequests: boolean;
  revision: EntityRevision;
  projectionRevision: ProjectionRevision;
  legacy: boolean;
}

/** Most notices one session keeps (Rust `SESSION_NOTICES_MAX`). */
export const SESSION_NOTICES_MAX = 16;

/** A harness notice about the session rather than one Turn. */
export interface SessionNotice {
  level: "info" | "warning" | "error";
  text: string;
  /** Who raised it, such as "codex config". */
  source: string | null;
}

export interface AcpChatPage {
  sessionConfig?: AcpSessionConfig | null;
  /** The current session's restored config while it has no live one. */
  restoredSessionConfig?: AcpRestoredSessionConfig | null;
  /** A stored choice the current session could not take back. */
  sessionConfigNotice?: AcpSessionConfigNotice | null;
  goal?: AcpGoalState;
  /** The current session's live runtime reports usage limits. */
  usage?: true;
  /** The session's activities and Undo, still undecoded: the panel that
   * shows them decodes them (`decodeSessionSurface`). */
  sessionSurface?: unknown;
  projectId: SageProjectId;
  chatId: SageChatId;
  title: string;
  projectionRevision: ProjectionRevision;
  currentSessionId: SageAgentSessionId | null;
  sessions: readonly AgentSessionProjection[];
  turns: readonly TurnProjection[];
  nextCursor: AcpPageCursor | null;
  /** The Chat this one was natively forked from (Rust `chat_lineage`). */
  forkedFrom?: SageChatId;
  /** The parent Turn the fork carries history through, inclusive. */
  forkedFromTurnId?: SageTurnId;
  /** The Chat continues a session imported from outside Sage. */
  importedHistory?: true;
  /**
   * Child-transcript invalidation epoch (opaque). It moves without the
   * projection revision, so child activity never stales a user action.
   */
  childProjectionEpoch?: number;
  /** The current session's newest notices, oldest first (at most 16). */
  sessionNotices?: readonly SessionNotice[];
}

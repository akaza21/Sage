import type { AcpImagePayload } from "./acpImage";
import type { AcpSessionConfig } from "./acpSessionConfig";
import type { BrowserOperationProjection } from "./browserOperation";
import type { FilePairMetadata } from "./agentDiff";
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

export type ToolPresentation =
  | {
      kind: "read" | "edit" | "delete";
      label: string;
      resources: readonly SafeResourceLocation[];
    }
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
    }
  | {
      kind: "subagent";
      label: string;
      activityId: SageActivityId | null;
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
        source?: "harness" | "browser";
        offeredOptionKinds?: Array<
          "allow_once" | "allow_always" | "reject_once" | "reject_always"
        > | null;
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

export interface AcpChatPage {
  sessionConfig?: AcpSessionConfig | null;
  projectId: SageProjectId;
  chatId: SageChatId;
  title: string;
  projectionRevision: ProjectionRevision;
  currentSessionId: SageAgentSessionId | null;
  sessions: readonly AgentSessionProjection[];
  turns: readonly TurnProjection[];
  nextCursor: AcpPageCursor | null;
}

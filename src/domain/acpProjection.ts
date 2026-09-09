import type { AcpImagePayload } from "./acpImage";
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
    };

export type ToolPresentation =
  | { kind: "generic"; label: string; summary: string | null }
  | {
      kind: "file" | "diff";
      label: string;
      resources: readonly SafeResourceLocation[];
    }
  | {
      kind: "command";
      label: string;
      summary: string | null;
      resources: readonly SafeResourceLocation[];
    }
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
  | PlainItem<"image", AcpImagePayload>
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

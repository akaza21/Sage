import type {
  ProjectGeneration,
  RuntimeEpoch,
  SageAgentSessionId,
  SageChatId,
  SageProjectId,
} from "./acp";

/** One live runtime whose harness usage limits are read. */
export interface AcpUsageRequest {
  projectId: SageProjectId;
  projectGeneration: ProjectGeneration;
  chatId: SageChatId;
  sessionId: SageAgentSessionId;
  runtimeEpoch: RuntimeEpoch;
}

export interface UsageWindow {
  label: string | null;
  usedPercent: number;
  windowMinutes: number | null;
  /** Unix seconds. */
  resetsAt: number | null;
}

export interface UsageSnapshot {
  plan: string | null;
  limitReached: boolean;
  windows: UsageWindow[];
}

export type AcpUsageState =
  | { kind: "unsupported" | "offline" | "unavailable" }
  | { kind: "usage"; snapshot: UsageSnapshot };

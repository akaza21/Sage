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

/** What the current session has spent so far, as its harness counts it. */
export interface SessionSpend {
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  totalTokens: number;
  /** US dollars at the harness's own prices. */
  costUsd: number | null;
}

export interface UsageSnapshot {
  plan: string | null;
  limitReached: boolean;
  windows: UsageWindow[];
  /** The session's own spend, for harnesses that report one. */
  session: SessionSpend | null;
}

export type AcpUsageState =
  | { kind: "unsupported" | "offline" | "unavailable" }
  | { kind: "usage"; snapshot: UsageSnapshot };

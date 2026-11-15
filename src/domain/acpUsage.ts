import type {
  ProjectGeneration,
  RuntimeEpoch,
  SageAgentSessionId,
  SageChatId,
  SageProjectId,
} from "./acp";

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
  session: SessionSpend | null;
}

export type AcpUsageState =
  | { kind: "unsupported" | "offline" | "unavailable" }
  | { kind: "usage"; snapshot: UsageSnapshot };

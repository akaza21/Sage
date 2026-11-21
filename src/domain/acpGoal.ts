export interface AcpGoalCapabilities {
  set: boolean;
  pause: boolean;
  resume: boolean;
  tokenBudget: boolean;
  clear: boolean;
}

export type GoalMutationAction = "set" | "pause" | "clear";

export function decodeGoalCapabilities(
  value: unknown,
): AcpGoalCapabilities | undefined {
  if (!value || typeof value !== "object") return undefined;
  const fields = ["set", "pause", "resume", "tokenBudget", "clear"] as const;
  const goal = value as Record<string, unknown>;
  if (fields.some((field) => typeof goal[field] !== "boolean"))
    return undefined;
  return Object.fromEntries(
    fields.map((field) => [field, goal[field]]),
  ) as unknown as AcpGoalCapabilities;
}

import { safeOrdinal } from "./guards";
import { utf8ByteLength } from "./text";

export type AcpGoalState =
  | { kind: "unsupported" }
  | { kind: "offline" }
  | { kind: "loading" }
  | { kind: "unavailable" }
  | { kind: "noGoal" }
  | { kind: "goal"; snapshot: AcpGoalSnapshot };

export interface AcpGoalSnapshot {
  objective: string;
  status:
    | "active"
    | "paused"
    | "blocked"
    | "usageLimited"
    | "budgetLimited"
    | "complete";
  tokenBudget: number | null;
  tokensUsed: number;
  timeUsedSeconds: number;
  createdAt: string | null;
  updatedAt: string | null;
  progress?: {
    iterations: number;
    setAt: number;
    tokensAtStart: number;
    lastReason: string | null;
  };
}

export interface AcpGoalChangedV1 {
  schemaVersion: 1;
  sageProjectId: string;
  sageChatId: string;
  sageAgentSessionId: string;
  runtimeEpoch: string;
}

const record = (value: unknown): value is Record<string, unknown> =>
  value !== null && typeof value === "object" && !Array.isArray(value);
const bounded = (value: unknown, max: number): value is string =>
  typeof value === "string" &&
  ![...value].some(
    (char) => char.charCodeAt(0) < 32 || char.charCodeAt(0) === 127,
  ) &&
  utf8ByteLength(value) <= max;
const counter = (value: unknown): value is number =>
  safeOrdinal(value) !== null;

export function decodeAcpGoal(value: unknown): AcpGoalState | null {
  if (!record(value)) return null;
  if (
    value.kind === "unsupported" ||
    value.kind === "offline" ||
    value.kind === "loading" ||
    value.kind === "unavailable" ||
    value.kind === "noGoal"
  ) {
    return { kind: value.kind };
  }
  if (value.kind !== "goal" || !record(value.snapshot)) return null;
  const goal = value.snapshot;
  if (
    typeof goal.objective !== "string" ||
    goal.objective.includes("\0") ||
    utf8ByteLength(goal.objective) > 16_000 ||
    goal.objective.length === 0 ||
    [...goal.objective].length > 4_000 ||
    ![
      "active",
      "paused",
      "blocked",
      "usageLimited",
      "budgetLimited",
      "complete",
    ].includes(String(goal.status)) ||
    (goal.tokenBudget !== null && !counter(goal.tokenBudget)) ||
    !counter(goal.tokensUsed) ||
    !counter(goal.timeUsedSeconds) ||
    (goal.createdAt !== null && !bounded(goal.createdAt, 64)) ||
    (goal.updatedAt !== null && !bounded(goal.updatedAt, 64)) ||
    (goal.progress !== undefined &&
      (!record(goal.progress) ||
        !counter(goal.progress.iterations) ||
        !counter(goal.progress.setAt) ||
        !counter(goal.progress.tokensAtStart) ||
        (goal.progress.lastReason !== null &&
          (typeof goal.progress.lastReason !== "string" ||
            goal.progress.lastReason.includes("\0") ||
            [...goal.progress.lastReason].length > 512))))
  )
    return null;
  return { kind: "goal", snapshot: goal as unknown as AcpGoalSnapshot };
}

export function decodeAcpGoalNotice(value: unknown): AcpGoalChangedV1 | null {
  if (!record(value) || value.schemaVersion !== 1) return null;
  for (const key of [
    "sageProjectId",
    "sageChatId",
    "sageAgentSessionId",
  ] as const) {
    if (!bounded(value[key], 512) || value[key].length === 0) return null;
  }
  if (
    typeof value.runtimeEpoch !== "string" ||
    !/^[1-9][0-9]*$/.test(value.runtimeEpoch)
  )
    return null;
  return value as unknown as AcpGoalChangedV1;
}

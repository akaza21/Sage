export interface AcpGoalCapabilities {
  observes: boolean;
  set: boolean;
  pause: boolean;
  resume: boolean;
  tokenBudget: boolean;
  clear: boolean;
}

export type GoalMutationAction =
  "set" | "pause" | "resume" | "tokenBudget" | "clear";

export function decodeGoalCapabilities(
  value: unknown,
): AcpGoalCapabilities | undefined {
  return record(value) &&
    ["observes", "set", "pause", "resume", "tokenBudget", "clear"].every(
      (operation) => typeof value[operation] === "boolean",
    )
    ? (value as unknown as AcpGoalCapabilities)
    : undefined;
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
const text = (value: unknown, max: number): value is string =>
  typeof value === "string" &&
  value.length <= max * 2 &&
  !value.includes("\0") &&
  [...value].length <= max;
const timestamp = (value: unknown): value is string | null =>
  value === null ||
  (text(value, 64) && !/\p{Cc}/u.test(value) && utf8ByteLength(value) <= 64);
const counter = (value: unknown): value is number =>
  safeOrdinal(value) !== null;

export function decodeAcpGoal(value: unknown): AcpGoalState | null {
  if (!record(value)) return null;
  switch (value.kind) {
    case "unsupported":
    case "offline":
    case "loading":
    case "unavailable":
    case "noGoal":
      return { kind: value.kind };
  }
  if (value.kind !== "goal" || !record(value.snapshot)) return null;
  const goal = value.snapshot;
  const progress = goal.progress;
  if (
    !text(goal.objective, 4_000) ||
    goal.objective.length === 0 ||
    ![
      "active",
      "paused",
      "blocked",
      "usageLimited",
      "budgetLimited",
      "complete",
    ].includes(goal.status as string) ||
    (goal.tokenBudget !== null && !counter(goal.tokenBudget)) ||
    !counter(goal.tokensUsed) ||
    !counter(goal.timeUsedSeconds) ||
    !timestamp(goal.createdAt) ||
    !timestamp(goal.updatedAt) ||
    (progress !== undefined &&
      (!record(progress) ||
        !counter(progress.iterations) ||
        !counter(progress.setAt) ||
        !counter(progress.tokensAtStart) ||
        (progress.lastReason !== null && !text(progress.lastReason, 512))))
  )
    return null;
  return { kind: "goal", snapshot: goal as unknown as AcpGoalSnapshot };
}

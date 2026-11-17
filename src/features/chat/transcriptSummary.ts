import type { TurnProjection } from "../../domain/acp";
import {
  elapsedSecondsBetween,
  formatElapsedSeconds,
} from "../../domain/duration";

export function turnHasActivities(
  turn: TurnProjection,
  workingItemCount: number,
): boolean {
  return (
    workingItemCount > 0 ||
    (turn.sessionActivities?.length ?? 0) > 0 ||
    (turn.browserOperations?.length ?? 0) > 0 ||
    (turn.browserOperationsCollapsed ?? 0) > 0
  );
}

export function formatWorkedDuration(
  startedAt: number,
  finishedAt: number | null,
): string | null {
  const seconds = elapsedSecondsBetween(startedAt, finishedAt);
  return seconds === null
    ? null
    : `Worked for ${formatElapsedSeconds(seconds)}`;
}

export function formatWorkingDuration(
  startedAt: number,
  nowAt: number,
): string | null {
  const seconds = elapsedSecondsBetween(startedAt, nowAt);
  return seconds === null
    ? null
    : `Working for ${formatElapsedSeconds(seconds)}`;
}

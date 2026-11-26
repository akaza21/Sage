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

/** "Worked for 3m 12s" once settled, "Working for 3s" while live. */
export function formatDuration(
  verb: "Worked" | "Working",
  startedAt: number,
  endedAt: number | null,
): string | null {
  const seconds = elapsedSecondsBetween(startedAt, endedAt);
  return seconds === null
    ? null
    : `${verb} for ${formatElapsedSeconds(seconds)}`;
}

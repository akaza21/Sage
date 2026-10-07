export type TranscriptActivityKind = "edit" | "execute" | "read";

export interface TranscriptActivityCount {
  kind: TranscriptActivityKind;
  count: number;
}

export type TranscriptActivityGroup = readonly TranscriptActivityCount[];

export function groupTranscriptActivities(
  activities: readonly (TranscriptActivityCount | null)[],
): readonly TranscriptActivityGroup[] {
  const groups: TranscriptActivityCount[][] = [];
  let current: TranscriptActivityCount[] = [];

  function finishGroup() {
    if (current.length > 0) groups.push(current);
    current = [];
  }

  for (const activity of activities) {
    if (
      activity === null ||
      (current.length > 0 && current[0]?.kind === "read") !==
        (activity?.kind === "read")
    ) {
      finishGroup();
      if (activity === null) continue;
    }
    const existing = current.find((entry) => entry.kind === activity.kind);
    if (existing) existing.count += activity.count;
    else current.push({ ...activity });
  }

  finishGroup();
  return groups;
}

export function formatWorkedDuration(
  startedAt: number,
  finishedAt: number | null,
): string | null {
  if (
    finishedAt === null ||
    !Number.isFinite(startedAt) ||
    !Number.isFinite(finishedAt) ||
    finishedAt < startedAt
  ) {
    return null;
  }
  const seconds = Math.round((finishedAt - startedAt) / 1_000);
  if (seconds < 60) return `Worked for ${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const hours = Math.floor(minutes / 60);
  const remainingMinutes = minutes % 60;
  if (hours > 0) {
    return remainingMinutes === 0
      ? `Worked for ${hours}h`
      : `Worked for ${hours}h ${remainingMinutes}m`;
  }
  const remainder = seconds % 60;
  return remainder === 0
    ? `Worked for ${minutes}m`
    : `Worked for ${minutes}m ${remainder}s`;
}

export function formatWorkingDuration(
  startedAt: number,
  nowAt: number,
): string | null {
  const worked = formatWorkedDuration(startedAt, nowAt);
  return worked ? worked.replace(/^Worked/, "Working") : null;
}

export function summarizeActivityKinds(
  activities: readonly TranscriptActivityCount[],
): string | null {
  if (activities.length === 0) return null;
  const phrases = activities.flatMap(({ kind, count }) => {
    if (!Number.isSafeInteger(count) || count <= 0) return [];
    if (kind === "edit") {
      return [count === 1 ? "Edited a file" : `Edited ${count} files`];
    }
    if (kind === "execute") return ["Ran commands"];
    return [`Explored ${count} ${count === 1 ? "file" : "files"}`];
  });
  if (phrases.length === 0) return null;
  return phrases
    .map((phrase, index) =>
      index === 0 ? phrase : `${phrase[0].toLowerCase()}${phrase.slice(1)}`,
    )
    .join(", ");
}

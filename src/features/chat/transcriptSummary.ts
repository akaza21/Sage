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

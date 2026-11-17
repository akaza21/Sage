const UNITS = [
  ["d", 86_400],
  ["h", 3_600],
  ["m", 60],
  ["s", 1],
] as const;

export function formatElapsedSeconds(totalSeconds: number): string {
  let remaining = Math.max(0, Math.round(totalSeconds));
  const parts: string[] = [];
  for (const [unit, size] of UNITS) {
    const value = Math.floor(remaining / size);
    remaining -= value * size;
    if (value) parts.push(`${value}${unit}`);
  }
  return parts.length ? parts.join(" ") : "0s";
}

export function elapsedSecondsBetween(
  startedAt: number,
  endedAt: number | null,
): number | null {
  if (
    endedAt === null ||
    !Number.isFinite(startedAt) ||
    !Number.isFinite(endedAt) ||
    endedAt < startedAt
  ) {
    return null;
  }
  return (endedAt - startedAt) / 1_000;
}

type LineChanges = { additions: number | null; deletions: number | null };

function lineChangeTotal(
  changes: readonly LineChanges[],
  side: keyof LineChanges,
) {
  return changes.reduce<number | null>(
    (total, change) =>
      total === null || change[side] === null ? null : total + change[side],
    0,
  );
}

export function FileChangeTotals({
  changes,
  incomplete = false,
}: {
  changes: readonly LineChanges[];
  incomplete?: boolean;
}) {
  if (!changes.length || incomplete) return null;
  const additions = lineChangeTotal(changes, "additions");
  const deletions = lineChangeTotal(changes, "deletions");
  if (additions === null && deletions === null) return null;
  return (
    <span className="acp-file-changes__totals" aria-label="Line changes">
      {additions !== null ? (
        <span className="acp-file-changes__additions">+{additions}</span>
      ) : null}
      {deletions !== null ? (
        <span className="acp-file-changes__deletions">-{deletions}</span>
      ) : null}
    </span>
  );
}

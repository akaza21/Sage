export function SessionWorkingPill({
  count,
  hidden,
}: {
  count: number;
  hidden: boolean;
}) {
  if (count === 0 || hidden) return null;
  return (
    <span
      className="session-working-pill"
      role="status"
      aria-label={`${count} ${count === 1 ? "working turn" : "working turns"}`}
    >
      {count} Working
    </span>
  );
}

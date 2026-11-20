const MONTH_MS = 30 * 86_400_000;

export function agentRailHoverAge(updatedAt: number | undefined, now: number) {
  if (!updatedAt || !Number.isFinite(updatedAt) || now < updatedAt) return "";
  const minutes = Math.floor((now - updatedAt) / 60_000);
  if (minutes < 1) return "now";
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}d`;
  const months = Math.floor((now - updatedAt) / MONTH_MS);
  return months < 12 ? `${months}mo` : `${Math.floor(months / 12)}y`;
}

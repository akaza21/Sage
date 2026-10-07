export function chatTimestamp(createdAt: number) {
  if (!Number.isFinite(createdAt) || createdAt <= 0) return null;
  const timestamp = new Date(createdAt);
  if (Number.isNaN(timestamp.getTime())) return null;
  return {
    dateTime: timestamp.toISOString(),
    label: timestamp.toISOString().slice(11, 16),
  };
}

export function dayTimeLabel(timestamp: number): string {
  const date = new Date(timestamp);
  const now = new Date();
  const sameDay =
    date.getFullYear() === now.getFullYear() &&
    date.getMonth() === now.getMonth() &&
    date.getDate() === now.getDate();
  const yesterday = new Date(now);
  yesterday.setDate(yesterday.getDate() - 1);
  const priorDay =
    date.getFullYear() === yesterday.getFullYear() &&
    date.getMonth() === yesterday.getMonth() &&
    date.getDate() === yesterday.getDate();
  const day = sameDay
    ? "Today"
    : priorDay
      ? "Yesterday"
      : new Intl.DateTimeFormat(undefined, {
          month: "short",
          day: "numeric",
          year:
            date.getFullYear() === now.getFullYear() ? undefined : "numeric",
        }).format(date);
  return `${day} ${new Intl.DateTimeFormat(undefined, {
    hour: "numeric",
    minute: "2-digit",
  }).format(date)}`;
}

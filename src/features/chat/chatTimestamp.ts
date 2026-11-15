export function chatTimestamp(createdAt: number) {
  if (!Number.isFinite(createdAt) || createdAt <= 0) return null;
  const timestamp = new Date(createdAt);
  if (Number.isNaN(timestamp.getTime())) return null;
  return {
    dateTime: timestamp.toISOString(),
    label: timeOfDayLabel(createdAt),
  };
}

export function dayTimeLabel(timestamp: number): string {
  const date = new Date(timestamp);
  const now = new Date();
  const localDay = date.toDateString();
  const sameDay = localDay === now.toDateString();
  const yesterday = new Date(now);
  yesterday.setDate(yesterday.getDate() - 1);
  const priorDay = localDay === yesterday.toDateString();
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
  return `${day} ${timeOfDayLabel(timestamp)}`;
}

export function timeOfDayLabel(timestamp: number): string {
  return new Intl.DateTimeFormat(undefined, {
    hour: "numeric",
    minute: "2-digit",
  }).format(timestamp);
}

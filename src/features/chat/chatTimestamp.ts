export function chatTimestamp(createdAt: number) {
  if (!Number.isFinite(createdAt) || createdAt <= 0) return null;
  const timestamp = new Date(createdAt);
  if (Number.isNaN(timestamp.getTime())) return null;
  return {
    dateTime: timestamp.toISOString(),
    label: timestamp.toISOString().slice(11, 16),
  };
}

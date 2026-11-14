const dayFormat = new Intl.DateTimeFormat(undefined, {
  weekday: "long",
  month: "long",
  day: "numeric",
  year: "numeric",
});

export function dayKey(time: number): string {
  const date = new Date(time);
  return `${date.getFullYear()}-${date.getMonth()}-${date.getDate()}`;
}

export function historyDayLabel(time: number, now = Date.now()): string {
  const today = new Date(now);
  const yesterday = new Date(
    today.getFullYear(),
    today.getMonth(),
    today.getDate() - 1,
  );
  const key = dayKey(time);
  const full = dayFormat.format(time);
  if (key === dayKey(now)) return `Today - ${full}`;
  if (key === dayKey(yesterday.getTime())) return `Yesterday - ${full}`;
  return full;
}

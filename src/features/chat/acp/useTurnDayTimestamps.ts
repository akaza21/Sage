import { useEffect, useMemo, useState } from "react";

import type { TurnProjection } from "../../../domain/acp";

function turnDayTimestamp(turn: TurnProjection): number | null {
  const prompt = turn.items.find(
    (item) => item.kind === "text" && item.payload.role === "user",
  );
  const timestamp = prompt?.createdAt ?? turn.createdAt;
  return Number.isFinite(timestamp) &&
    timestamp > 0 &&
    !Number.isNaN(new Date(timestamp).getTime())
    ? timestamp
    : null;
}

function sameCalendarDay(left: number, right: number): boolean {
  const leftDate = new Date(left);
  const rightDate = new Date(right);
  return (
    leftDate.getFullYear() === rightDate.getFullYear() &&
    leftDate.getMonth() === rightDate.getMonth() &&
    leftDate.getDate() === rightDate.getDate()
  );
}

export function useTurnDayTimestamps(turns: readonly TurnProjection[]) {
  const [renderedAt, setRenderedAt] = useState(() => Date.now());
  useEffect(() => {
    const midnight = new Date(renderedAt);
    midnight.setHours(24, 0, 0, 0);
    const timer = window.setTimeout(
      () => setRenderedAt(Date.now()),
      Math.max(1, midnight.getTime() - Date.now()),
    );
    return () => window.clearTimeout(timer);
  }, [renderedAt]);
  return useMemo(() => {
    const timestamps: (number | null)[] = [];
    let prior: number | null = null;
    for (const turn of turns) {
      const timestamp = turnDayTimestamp(turn);
      const showDay =
        timestamp !== null &&
        (prior === null || !sameCalendarDay(timestamp, prior));
      timestamps.push(showDay ? timestamp : null);
      if (timestamp !== null) prior = timestamp;
    }
    return timestamps;
  }, [turns]);
}

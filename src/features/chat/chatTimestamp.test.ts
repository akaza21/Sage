import { afterEach, expect, it, vi } from "vitest";
import { chatTimestamp, dayTimeLabel } from "./chatTimestamp";

it("uses the local clock for message and activity labels while retaining an ISO instant", () => {
  const instant = new Date(2026, 9, 30, 6, 5).getTime();
  const time = chatTimestamp(instant)!;
  expect(time.dateTime).toBe(new Date(instant).toISOString());
  expect(time.label).toMatch(/6:05/);
  expect(dayTimeLabel(instant)).toContain(time.label);
});

it.each([NaN, Infinity, -1, 0, 9e20])(
  "omits an invalid timestamp %s",
  (instant) => {
    expect(chatTimestamp(instant)).toBeNull();
  },
);

afterEach(() => vi.useRealTimers());

it("compares local calendar days across midnight and year boundaries", () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date(2026, 0, 1, 0, 5));
  expect(dayTimeLabel(new Date(2026, 0, 1, 0, 1).getTime())).toMatch(/^Today /);
  expect(dayTimeLabel(new Date(2025, 11, 31, 23, 59).getTime())).toMatch(
    /^Yesterday /,
  );
  expect(dayTimeLabel(new Date(2025, 11, 30, 23, 59).getTime())).toContain(
    "2025",
  );
});

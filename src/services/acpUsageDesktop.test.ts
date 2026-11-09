import { expect, it } from "vitest";
import { decodeUsageState } from "./acpUsageDesktop";

it("decodes the native usage snapshot and rejects anything out of bounds", () => {
  const usage = {
    kind: "usage",
    snapshot: {
      plan: "Pro",
      limitReached: false,
      windows: [
        {
          label: null,
          usedPercent: 42,
          windowMinutes: 300,
          resetsAt: 1_762_900_000,
        },
        { label: "Spark", usedPercent: 7, windowMinutes: null, resetsAt: null },
      ],
    },
  };
  expect(decodeUsageState(usage)).toEqual(usage);
  expect(decodeUsageState({ kind: "unsupported" })).toEqual({
    kind: "unsupported",
  });

  const window = usage.snapshot.windows[0];
  for (const bad of [
    { ...window, usedPercent: 101 },
    { ...window, usedPercent: 4.5 },
    { ...window, resetsAt: -1 },
    { ...window, label: "x".repeat(129) },
  ])
    expect(
      decodeUsageState({
        kind: "usage",
        snapshot: { ...usage.snapshot, windows: [bad] },
      }),
    ).toBeNull();
  expect(
    decodeUsageState({
      kind: "usage",
      snapshot: { ...usage.snapshot, windows: Array(9).fill(window) },
    }),
  ).toBeNull();
  expect(decodeUsageState({ kind: "limits" })).toBeNull();
});

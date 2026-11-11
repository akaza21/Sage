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
      session: null,
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

it("decodes a session's spend and rejects a malformed one", () => {
  const session = {
    inputTokens: 1_200,
    outputTokens: 340,
    cacheReadTokens: 800,
    cacheWriteTokens: 60,
    totalTokens: 2_400,
    costUsd: 0.0125,
  };
  const usage = {
    kind: "usage",
    snapshot: { plan: null, limitReached: false, windows: [], session },
  };
  expect(decodeUsageState(usage)).toEqual(usage);
  const unpriced = {
    ...usage,
    snapshot: { ...usage.snapshot, session: { ...session, costUsd: null } },
  };
  expect(decodeUsageState(unpriced)).toEqual(unpriced);
  for (const bad of [
    { ...session, costUsd: -0.5 },
    { ...session, costUsd: "0.01" },
    { ...session, inputTokens: 1.5 },
    { ...session, totalTokens: -1 },
    { ...session, cacheWriteTokens: undefined },
  ])
    expect(
      decodeUsageState({
        kind: "usage",
        snapshot: { ...usage.snapshot, session: bad },
      }),
    ).toBeNull();
  expect(
    decodeUsageState({
      kind: "usage",
      snapshot: { plan: null, limitReached: false, windows: [] },
    }),
  ).toBeNull();
});

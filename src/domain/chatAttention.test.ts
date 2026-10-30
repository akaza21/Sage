import { attention } from "./chatAttention.fixture";
import { expect, it } from "vitest";
import { attentionStatus, attentionUnread } from "./chatAttention";

it("attention preserves T3 never-visited and request precedence", () => {
  expect(attentionUnread(attention(), false)).toBe(false);
  expect(attentionUnread(attention({ terminalState: "failed" }), false)).toBe(
    true,
  );
  expect(attentionUnread(attention({ forcedUnreadPosition: "1" }), false)).toBe(
    true,
  );
  expect(attentionUnread(attention({ readPosition: "0" }), false)).toBe(true);
  expect(attentionUnread(attention({ readPosition: "1" }), false)).toBe(false);
  expect(attentionUnread(attention({ terminalState: "failed" }), true)).toBe(
    false,
  );
  expect(
    attentionStatus(
      attention({
        pendingApprovals: 1,
        pendingRequests: 1,
        running: true,
        terminalState: "failed",
      }),
    ),
  ).toEqual({ status: "waiting", approval: true });
  expect(
    attentionStatus(attention({ pendingRequests: 1, running: true })),
  ).toEqual({ status: "waiting", approval: false });
  expect(
    attentionStatus(attention({ running: true, terminalState: "failed" }))
      .status,
  ).toBe("working");
  expect(attentionStatus(attention({ terminalState: "failed" })).status).toBe(
    "failed",
  );
});

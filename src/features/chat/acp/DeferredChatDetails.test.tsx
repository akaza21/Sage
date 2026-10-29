import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { AcpItemIdentity } from "../../../domain/acp";
import {
  DeferredInlineSavedDiff,
  DeferredTurnFileActivity,
} from "./DeferredChatDetails";
import { snapshotPage } from "../../../app/agentDiff.fixture";

const requests = vi.hoisted(() => ({
  diff: vi.fn(),
  files: vi.fn(),
}));
vi.mock("./TurnFileActivity", () => {
  requests.files();
  return {
    TurnFileActivity: () => (
      <section aria-label="Edited files">Edit card ready</section>
    ),
  };
});
vi.mock("./InlineSavedDiff", () => {
  requests.diff();
  return {
    InlineSavedDiff: () => <pre aria-label="Saved diff">Diff detail ready</pre>,
  };
});
afterEach(cleanup);

it("keeps the edit card unloaded until a turn reports edits and lazy-loads the disclosure", async () => {
  const page = snapshotPage();
  const { rerender } = render(
    <DeferredTurnFileActivity
      page={page}
      turn={{ ...page.turns[0], items: [] }}
    />,
  );
  expect(requests.files).not.toHaveBeenCalled();
  rerender(<DeferredTurnFileActivity page={page} turn={page.turns[0]} />);
  expect(
    await screen.findByRole("region", { name: "Edited files" }),
  ).toHaveTextContent("Edit card ready");
  expect(requests.files).toHaveBeenCalledOnce();
  rerender(
    <DeferredTurnFileActivity
      page={page}
      turn={{ ...page.turns[0], items: [] }}
    />,
  );
  expect(screen.queryByRole("region", { name: "Edited files" })).toBeNull();
});

const identity = {
  id: "item-1",
  projectId: "project-1",
  chatId: "chat-1",
  sessionId: "session-1",
  turnId: "turn-1",
  runtimeEpoch: "1",
  ordinal: 1,
  origin: "primary",
  activityId: null,
  lifecycle: "completed",
  revision: "1",
  createdAt: 1,
  updatedAt: 2,
  terminalAt: 2,
} as AcpItemIdentity;

it("requests saved-diff rendering only when the inline preview mounts", async () => {
  expect(requests.diff).not.toHaveBeenCalled();
  render(
    <DeferredInlineSavedDiff
      resource={{ rootId: "root", relativePath: "a.ts" }}
      diffRef={{
        projectId: identity.projectId,
        chatId: identity.chatId,
        sessionId: identity.sessionId,
        runtimeEpoch: identity.runtimeEpoch,
        turnId: identity.turnId,
        itemId: identity.id,
        diffOrdinal: 0,
        snapshotRevision: "1",
        rootId: "root",
        relativePath: "a.ts",
        contentDigest: "a".repeat(64),
      }}
    />,
  );
  expect(await screen.findByLabelText("Saved diff")).toHaveTextContent(
    "Diff detail ready",
  );
  expect(requests.diff).toHaveBeenCalledTimes(1);
});

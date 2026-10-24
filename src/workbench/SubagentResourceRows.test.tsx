import {
  act,
  cleanup,
  render,
  renderHook,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import type { AcpChatPage } from "../domain/acpProjection";
import type { AgentArtifactOwner } from "../domain/agentArtifact";
import { SubagentResourceRows, useChildEpoch } from "./SubagentResourceRows";

vi.mock("../ports/nativeInvoke", async (importOriginal) => ({
  ...((await importOriginal()) as object),
  isTauriRuntime: () => true,
}));
const childNotices = vi.hoisted(
  () => new Map<string, (epoch: number) => void>(),
);
vi.mock("../services/acpChildTranscript", async (importOriginal) => ({
  ...((await importOriginal()) as object),
  subscribeChildProjectionChanges: async (
    projectId: string,
    chatId: string,
    onEpoch: (epoch: number) => void,
  ) => {
    childNotices.set(projectId + "/" + chatId, onEpoch);
    return () => undefined;
  },
}));

afterEach(() => {
  cleanup();
  childNotices.clear();
});

it("ignores a late child notice from a torn-down chat listener", async () => {
  const page = (chatId: string) =>
    ({ projectId: "project", chatId, turns: [] }) as unknown as AcpChatPage;
  const view = renderHook(({ chat }) => useChildEpoch(page(chat)), {
    initialProps: { chat: "a" },
  });
  await waitFor(() => expect(view.result.current).toBe(0));
  const staleNotice = childNotices.get("project/a")!;
  view.rerender({ chat: "b" });
  await waitFor(() => expect(view.result.current).toBe(0));
  act(() => childNotices.get("project/b")!(4));
  expect(view.result.current).toBe(4);
  act(() => staleNotice(9));
  expect(view.result.current).toBe(4);
});

const agents = [
  {
    id: "spawn:child",
    label: "Reviewer",
    status: "working" as const,
    target: {
      owner: {
        projectId: "project-a",
        chatId: "chat:a",
        sessionId: "session-1",
      } as AgentArtifactOwner,
      itemId: "spawn",
      agentKey: "child",
    },
  },
];

it("focuses the first dialog control in document order when lazy rows arrive", () => {
  const { rerender } = render(
    <div role="dialog" tabIndex={-1}>
      <button>Sources</button>
    </div>,
  );
  screen.getByRole("dialog").focus();
  rerender(
    <div role="dialog" tabIndex={-1}>
      <button>Sources</button>
      <SubagentResourceRows agents={agents} onSelect={() => {}} />
    </div>,
  );
  expect(screen.getByRole("button", { name: "Sources" })).toHaveFocus();
});

it("does not move focus from Sources when lazy rows arrive", () => {
  const { rerender } = render(
    <div role="dialog" tabIndex={-1}>
      <button>Sources</button>
    </div>,
  );
  screen.getByRole("button", { name: "Sources" }).focus();
  rerender(
    <div role="dialog" tabIndex={-1}>
      <button>Sources</button>
      <SubagentResourceRows agents={agents} onSelect={() => {}} />
    </div>,
  );
  expect(screen.getByRole("button", { name: "Sources" })).toHaveFocus();
});

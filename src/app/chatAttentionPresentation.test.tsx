import {
  act,
  cleanup,
  render,
  renderHook,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { attention } from "../domain/chatAttention.fixture";
import type { ChatAttentionV1 } from "../domain/chatAttention";
import { page } from "../features/chat/acp/acpProjection.fixture";
import {
  chatAttention,
  observeAttentionBeforeDetail,
} from "../services/chatAttention";
import { usePresentedAttention } from "./chatAttentionPresentation";
import { chatAttentionStore } from "./chatAttentionStore";
import { useWorkbenchStore } from "./store";
import { AcpChatProjection } from "../features/chat/acp/AcpChatProjection";

const initial = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  chatAttentionStore.disconnect();
  vi.restoreAllMocks();
  useWorkbenchStore.setState(initial, true);
});

it("attention acknowledges a successfully presented empty foreground detail", async () => {
  vi.spyOn(document, "hasFocus").mockReturnValue(true);
  const empty = attention({
    activityPosition: "0",
    revision: "0",
    terminalPosition: null,
    terminalState: null,
    turnState: null,
  });
  vi.spyOn(chatAttention, "get").mockResolvedValue([empty]);
  vi.spyOn(chatAttention, "subscribe").mockResolvedValue(() => undefined);
  const ack = vi
    .spyOn(chatAttention, "ack")
    .mockResolvedValue({ ...empty, readPosition: "0", revision: "1" });
  const detail = await observeAttentionBeforeDetail(
    "project-1",
    "chat-1",
    async () => ({ ...page("completed"), turns: [] }),
  );
  await chatAttentionStore.connect([
    { projectId: "project-1", chatIds: ["chat-1"] },
  ]);
  render(
    <AcpChatProjection
      page={detail}
      active
      focused
      emptyState={<p>Start a chat</p>}
    />,
  );
  await waitFor(() =>
    expect(ack).toHaveBeenCalledWith("project-1", "chat-1", "0"),
  );
});

it("attention integration preserves native titles without token-driven rail recency", () => {
  useWorkbenchStore.setState({
    project: { id: "project-1", name: "Project", roots: [] },
    projectTransitioning: false,
    tabs: [
      {
        id: "chat-1",
        kind: "chat",
        title: "Chat: Chat",
        position: 0,
        pinned: false,
        dirty: false,
        updatedAt: 1,
      },
    ],
  });
  const { rerender } = renderHook(
    ({ detail }) => usePresentedAttention(detail, false),
    { initialProps: { detail: page("running") } },
  );
  expect(useWorkbenchStore.getState().tabs[0]).toMatchObject({
    title: "Durable chat",
    updatedAt: 2,
  });
  const tabs = useWorkbenchStore.getState().tabs;
  const next = page("running");
  next.turns[0].updatedAt = 999;
  rerender({ detail: next });
  expect(useWorkbenchStore.getState().tabs).toBe(tabs);
});

it("attention acknowledges a presented page after delayed summary bootstrap", async () => {
  vi.spyOn(document, "hasFocus").mockReturnValue(true);
  const get = vi.spyOn(chatAttention, "get").mockResolvedValue([attention()]);
  const detail = await observeAttentionBeforeDetail(
    "project-1",
    "chat-1",
    async () => page("completed"),
  );
  let finish!: (rows: ChatAttentionV1[]) => void;
  get.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  vi.spyOn(chatAttention, "subscribe").mockResolvedValue(() => undefined);
  const ack = vi
    .spyOn(chatAttention, "ack")
    .mockResolvedValue(attention({ readPosition: "1", revision: "4" }));
  const connecting = chatAttentionStore.connect([
    { projectId: "project-1", chatIds: ["chat-1"] },
  ]);
  await Promise.resolve();
  renderHook(() => usePresentedAttention(detail, true));
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  expect(ack).not.toHaveBeenCalled();
  await act(async () => {
    finish([attention({ activityPosition: "3", revision: "3" })]);
    await connecting;
  });
  await waitFor(() =>
    expect(ack).toHaveBeenCalledWith("project-1", "chat-1", "1"),
  );
  expect(ack).toHaveBeenCalledTimes(1);
});

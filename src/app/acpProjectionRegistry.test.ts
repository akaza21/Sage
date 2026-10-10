import { afterEach, describe, expect, it, vi } from "vitest";

import {
  MAX_VISIBLE_ACP_PROJECTION_SLOTS,
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
  hasActiveAcpTurn,
  occupiedAcpProjectionSlotCount,
  pinAcpProjectionIds,
  releaseAcpProjectionStore,
  subscribeAcpTurnActivity,
} from "./acpProjectionRegistry";
import { useAcpProjectionStore } from "./acpProjectionStore";

afterEach(() => {
  useAcpProjectionStore.getState().clear();
  clearAcpProjectionRegistry();
});

describe("ACP projection registry", () => {
  it("counts only current-project visible turns and drops released slots without allocating another", () => {
    const live = acpProjectionStoreFor("chat-a");
    live.setState({
      scope: {
        projectId: "project-a",
        projectGeneration: "7",
        chatId: "chat-a",
      },
      page: {
        projectId: "project-a",
        currentSessionId: "session-a",
        turns: [{ sessionId: "session-a", state: "running" }],
      },
    } as never);
    expect(hasActiveAcpTurn("project-a", 7)).toBe(false);
    pinAcpProjectionIds(["chat-a"]);
    expect(hasActiveAcpTurn("project-a", 7)).toBe(true);
    expect(hasActiveAcpTurn("project-b", 7)).toBe(false);
    expect(hasActiveAcpTurn("project-a", 8)).toBe(false);
    expect(hasActiveAcpTurn(null, 7)).toBe(false);
    releaseAcpProjectionStore("chat-a");
    expect(hasActiveAcpTurn("project-a", 7)).toBe(false);
    expect(occupiedAcpProjectionSlotCount()).toBe(0);
  });

  it("notifies store and coalesced membership changes and unsubscribes evicted slots and observers", async () => {
    await Promise.resolve();
    const notify = vi.fn();
    const unsubscribe = subscribeAcpTurnActivity(notify);
    const first = acpProjectionStoreFor("chat-a");
    acpProjectionStoreFor("chat-b");
    pinAcpProjectionIds(["chat-a", "chat-b"]);
    await Promise.resolve();
    expect(notify).toHaveBeenCalledTimes(1);
    first.setState({ error: "Unavailable" });
    expect(notify).toHaveBeenCalledTimes(2);
    pinAcpProjectionIds(["chat-b", "chat-c"]);
    acpProjectionStoreFor("chat-c");
    await Promise.resolve();
    expect(notify).toHaveBeenCalledTimes(3);
    first.setState({ error: null });
    expect(notify).toHaveBeenCalledTimes(3);
    unsubscribe();
    releaseAcpProjectionStore("chat-b");
    useAcpProjectionStore.setState({ error: "Unavailable" });
    await Promise.resolve();
    expect(notify).toHaveBeenCalledTimes(3);
  });

  it("keeps the unbound store on the singleton and isolates two visible chats", () => {
    expect(acpProjectionStoreFor()).toBe(useAcpProjectionStore);
    const first = acpProjectionStoreFor("chat-a");
    const second = acpProjectionStoreFor("chat-b");
    expect(first).not.toBe(useAcpProjectionStore);
    expect(second).not.toBe(first);
    expect(acpProjectionStoreFor("chat-a")).toBe(first);
    expect(MAX_VISIBLE_ACP_PROJECTION_SLOTS).toBe(2);
  });

  it("evicts the oldest visible slot instead of mounting a third page", () => {
    const first = acpProjectionStoreFor("chat-a");
    const second = acpProjectionStoreFor("chat-b");
    const third = acpProjectionStoreFor("chat-c");
    expect(third).not.toBe(first);
    expect(third).not.toBe(second);
    expect(acpProjectionStoreFor("chat-b")).toBe(second);
    expect(acpProjectionStoreFor("chat-c")).toBe(third);
    releaseAcpProjectionStore("chat-b");
    releaseAcpProjectionStore("chat-c");
    expect(acpProjectionStoreFor("chat-b")).not.toBe(second);
  });

  it("keeps a still-visible store when acquiring a third chat while the remaining pane is pinned", () => {
    const first = acpProjectionStoreFor("chat-a");
    acpProjectionStoreFor("chat-b");
    pinAcpProjectionIds(["chat-a", "chat-c"]);
    const third = acpProjectionStoreFor("chat-c");
    expect(acpProjectionStoreFor("chat-a")).toBe(first);
    expect(acpProjectionStoreFor("chat-c")).toBe(third);
    expect(third).not.toBe(first);
    expect(third).not.toBe(useAcpProjectionStore);
  });

  it("keeps the incoming remaining pane when the dropped id is the oldest slot", () => {
    acpProjectionStoreFor("chat-a");
    const second = acpProjectionStoreFor("chat-b");
    pinAcpProjectionIds(["chat-c", "chat-b"]);
    const third = acpProjectionStoreFor("chat-c");
    expect(acpProjectionStoreFor("chat-b")).toBe(second);
    expect(acpProjectionStoreFor("chat-c")).toBe(third);
    expect(third).not.toBe(second);
    expect(third).not.toBe(useAcpProjectionStore);
  });

  it("still evicts the oldest slot when nothing is pinned", () => {
    const first = acpProjectionStoreFor("chat-a");
    const second = acpProjectionStoreFor("chat-b");
    const third = acpProjectionStoreFor("chat-c");
    expect(third).not.toBe(first);
    expect(third).not.toBe(second);
    expect(third).not.toBe(useAcpProjectionStore);
    expect(acpProjectionStoreFor("chat-b")).toBe(second);
    expect(acpProjectionStoreFor("chat-c")).toBe(third);
  });

  it("ignores pin ids beyond the two visible slots", () => {
    const first = acpProjectionStoreFor("chat-a");
    const second = acpProjectionStoreFor("chat-b");
    pinAcpProjectionIds(["chat-c", "chat-d", "chat-a"]);
    const third = acpProjectionStoreFor("chat-c");
    expect(third).not.toBe(first);
    expect(third).not.toBe(useAcpProjectionStore);
    expect(acpProjectionStoreFor("chat-b")).toBe(second);
    expect(acpProjectionStoreFor("chat-c")).toBe(third);
  });

  it("refuses a third allocation when every slot is still pinned", () => {
    const first = acpProjectionStoreFor("chat-a");
    const second = acpProjectionStoreFor("chat-b");
    pinAcpProjectionIds(["chat-a", "chat-b"]);
    const refused = acpProjectionStoreFor("chat-c");
    expect(refused).toBe(useAcpProjectionStore);
    expect(acpProjectionStoreFor("chat-a")).toBe(first);
    expect(acpProjectionStoreFor("chat-b")).toBe(second);
  });
});

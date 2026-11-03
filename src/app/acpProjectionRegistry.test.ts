import { afterEach, describe, expect, it } from "vitest";

import {
  MAX_VISIBLE_ACP_PROJECTION_SLOTS,
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
  pinAcpProjectionIds,
  pinnedAcpProjectionIds,
  releaseAcpProjectionStore,
} from "./acpProjectionRegistry";
import { useAcpProjectionStore } from "./acpProjectionStore";

afterEach(() => {
  clearAcpProjectionRegistry();
});

describe("ACP projection registry", () => {
  it("exposes a bounded copy of pinned owners for sleeping-workspace continuity", () => {
    pinAcpProjectionIds(["chat-a", "chat-b", "chat-c"]);
    const snapshot = pinnedAcpProjectionIds();
    expect(snapshot).toEqual(["chat-a", "chat-b"]);
    snapshot.pop();
    expect(pinnedAcpProjectionIds()).toEqual(["chat-a", "chat-b"]);
    clearAcpProjectionRegistry();
    expect(pinnedAcpProjectionIds()).toEqual([]);
  });
  it("retains a pinned projection through a subscription cleanup (R3-13)", () => {
    pinAcpProjectionIds(["chat-a"]);
    const visible = acpProjectionStoreFor("chat-a");
    releaseAcpProjectionStore("chat-a");
    expect(acpProjectionStoreFor("chat-a")).toBe(visible);
    pinAcpProjectionIds([]);
    releaseAcpProjectionStore("chat-a");
    expect(acpProjectionStoreFor("chat-a")).not.toBe(visible);
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

import { describe, expect, it, vi } from "vitest";
import type {
  NotificationChat,
  NotificationPort,
} from "../ports/nativeNotificationPort";
import {
  listenNotificationNavigation,
  observeNotificationVisibility,
} from "./notificationLifecycle";

function port(): NotificationPort {
  return {
    getPreference: vi.fn(async () => "off" as const),
    setPreference: vi.fn(async (value) => value),
    beginVisibility: vi.fn(async () => 1),
    reportVisibility: vi.fn(async () => {}),
    takeChat: vi.fn(async () => null),
    listenOpen: vi.fn(async () => () => {}),
  };
}
const flush = async () => {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
};
function source() {
  const events = new EventTarget();
  return Object.assign(events, {
    visibilityState: "visible" as DocumentVisibilityState,
  });
}

describe("notification visibility", () => {
  it("reports both displayed panes, document changes and cleanup, without polling or duplicate reports", async () => {
    const native = port();
    const document = source();
    const stop = observeNotificationVisibility(
      native,
      ["left", "right"],
      true,
      document,
    );
    await flush();
    expect(native.reportVisibility).toHaveBeenLastCalledWith({
      generation: 1,
      revision: 1,
      documentVisible: true,
      chatIds: ["left", "right"],
    });
    document.dispatchEvent(new Event("visibilitychange"));
    expect(native.reportVisibility).toHaveBeenCalledTimes(1);
    document.visibilityState = "hidden";
    document.dispatchEvent(new Event("visibilitychange"));
    expect(native.reportVisibility).toHaveBeenLastCalledWith({
      generation: 1,
      revision: 2,
      documentVisible: false,
      chatIds: [],
    });
    document.visibilityState = "visible";
    document.dispatchEvent(new Event("visibilitychange"));
    stop();
    expect(native.reportVisibility).toHaveBeenLastCalledWith({
      generation: 1,
      revision: 4,
      documentVisible: false,
      chatIds: [],
    });
    document.dispatchEvent(new Event("visibilitychange"));
    expect(native.reportVisibility).toHaveBeenCalledTimes(4);
  });

  it("never reports a hidden Agent or a disposed renderer as visible", async () => {
    const native = port();
    let begin!: (generation: number) => void;
    vi.mocked(native.beginVisibility).mockReturnValue(
      new Promise((resolve) => {
        begin = resolve;
      }),
    );
    const stop = observeNotificationVisibility(
      native,
      ["chat"],
      true,
      source(),
    );
    stop();
    begin(4);
    await flush();
    expect(native.reportVisibility).toHaveBeenLastCalledWith({
      generation: 4,
      revision: 1,
      documentVisible: false,
      chatIds: [],
    });
    vi.mocked(native.beginVisibility).mockResolvedValue(5);
    const stopHidden = observeNotificationVisibility(
      native,
      ["chat"],
      false,
      source(),
    );
    await flush();
    expect(native.reportVisibility).toHaveBeenLastCalledWith({
      generation: 5,
      revision: 1,
      documentVisible: false,
      chatIds: [],
    });
    stopHidden();
  });

  it("contains native failure and rejects neither cleanup nor visibility events", async () => {
    const native = port();
    vi.mocked(native.reportVisibility).mockRejectedValue(
      new Error("stale renderer"),
    );
    const stop = observeNotificationVisibility(
      native,
      ["chat"],
      true,
      source(),
    );
    await flush();
    stop();
    await flush();
    expect(native.reportVisibility).toHaveBeenCalledTimes(2);
  });
});

describe("notification navigation", () => {
  it("registers before draining a retained click and opens the exact native chat", async () => {
    const native = port();
    const target = { projectId: "project", chatId: "chat" };
    vi.mocked(native.takeChat).mockResolvedValueOnce(target);
    const open = vi.fn(async () => {});
    const errors = vi.fn();
    const stop = vi.fn();
    vi.mocked(native.listenOpen).mockResolvedValue(stop);
    const navigation = listenNotificationNavigation(
      native,
      () => true,
      open,
      errors,
    );
    expect(native.takeChat).not.toHaveBeenCalled();
    await flush();
    expect(open).toHaveBeenCalledWith(target);
    expect(errors).not.toHaveBeenCalled();
    navigation.dispose();
    expect(stop).toHaveBeenCalledOnce();
  });

  it("waits for conflicting project navigation and coalesces clicks while opening", async () => {
    const native = port();
    let ready = false;
    let finish!: () => void;
    const open = vi.fn(
      (_chat: NotificationChat) =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    vi.mocked(native.takeChat).mockResolvedValueOnce({
      projectId: "p",
      chatId: "c",
    });
    const navigation = listenNotificationNavigation(
      native,
      () => ready,
      open,
      vi.fn(),
    );
    await flush();
    expect(native.takeChat).not.toHaveBeenCalled();
    ready = true;
    navigation.wake();
    await flush();
    for (let i = 0; i < 100; i++) navigation.wake();
    expect(native.takeChat).toHaveBeenCalledTimes(1);
    finish();
    await flush();
    expect(native.takeChat).toHaveBeenCalledTimes(2);
    expect(open).toHaveBeenCalledOnce();
    navigation.dispose();
  });

  it("cleans up a late subscription and contains unavailable or deleted chat failures", async () => {
    const native = port();
    let listen!: (stop: () => void) => void;
    vi.mocked(native.listenOpen).mockReturnValue(
      new Promise((resolve) => {
        listen = resolve;
      }),
    );
    const error = vi.fn();
    const open = vi.fn(async () => {});
    const navigation = listenNotificationNavigation(
      native,
      () => true,
      open,
      error,
    );
    navigation.dispose();
    const stop = vi.fn();
    listen(stop);
    await flush();
    expect(stop).toHaveBeenCalledOnce();
    expect(open).not.toHaveBeenCalled();
    const second = port();
    vi.mocked(second.takeChat).mockRejectedValue(new Error("unavailable"));
    const next = listenNotificationNavigation(second, () => true, open, error);
    await flush();
    expect(error).toHaveBeenCalledOnce();
    next.dispose();
  });
});

it("retains one click when a project transition starts during the native read", async () => {
  const native = port();
  let ready = true;
  let finish!: (chat: NotificationChat) => void;
  vi.mocked(native.takeChat).mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  const open = vi.fn(async () => {});
  const navigation = listenNotificationNavigation(
    native,
    () => ready,
    open,
    vi.fn(),
  );
  await flush();
  ready = false;
  finish({ projectId: "p", chatId: "c" });
  await flush();
  expect(open).not.toHaveBeenCalled();
  ready = true;
  navigation.wake();
  await flush();
  expect(open).toHaveBeenCalledWith({ projectId: "p", chatId: "c" });
  expect(native.takeChat).toHaveBeenCalledOnce();
  navigation.dispose();
});

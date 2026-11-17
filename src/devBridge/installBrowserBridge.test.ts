import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { isTauriRuntime } from "../ports/nativeInvoke";
import { installBrowserBridge } from "./installBrowserBridge";

class FakeSocket extends EventTarget {
  static readonly OPEN = 1;
  static latest: FakeSocket;
  readyState = 0;
  bufferedAmount = 0;
  binaryType = "blob";
  sent: { id: number; cmd: string; args: Record<string, unknown> }[] = [];
  constructor(readonly url: string) {
    super();
    FakeSocket.latest = this;
  }
  send(data: string) {
    this.sent.push(JSON.parse(data));
  }
  open() {
    this.readyState = 1;
    this.dispatchEvent(new Event("open"));
  }
  receiveBinary(data: ArrayBuffer) {
    this.dispatchEvent(new MessageEvent("message", { data }));
  }
  receive(frame: unknown) {
    this.dispatchEvent(
      new MessageEvent("message", { data: JSON.stringify(frame) }),
    );
  }
  close() {
    this.readyState = 3;
    this.dispatchEvent(new Event("close"));
  }
}

beforeEach(() => {
  sessionStorage.clear();
  history.replaceState(null, "", "/?bridge=32123#" + "a".repeat(64));
  vi.stubGlobal("WebSocket", FakeSocket);
});
afterEach(() => {
  FakeSocket.latest?.close();
  document.body.replaceChildren();
  vi.useRealTimers();
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  Reflect.deleteProperty(window, "__TAURI_EVENT_PLUGIN_INTERNALS__");
  vi.unstubAllGlobals();
});

async function connect() {
  const installed = installBrowserBridge();
  expect(location.hash).toBe("");
  const socket = FakeSocket.latest;
  socket.open();
  socket.receive({ session: "b".repeat(64) });
  await installed;
  return socket;
}

it("strips the fragment immediately and resolves or rejects invokes by id", async () => {
  const socket = await connect();
  expect(isTauriRuntime()).toBe(true);
  const first = invoke("get_bootstrap_snapshot", { projectId: "one" });
  const second = invoke("rename_chat", { chatId: "missing" });
  const rejected = expect(second).rejects.toEqual("chat missing");
  const [one, two] = socket.sent.slice(-2);
  socket.receive({ id: two.id, err: "chat missing" });
  socket.receive({ id: one.id, ok: { projectId: "one" } });
  await expect(first).resolves.toEqual({ projectId: "one" });
  await rejected;
});

it("delivers real API event listeners and unlisten stops delivery", async () => {
  const socket = await connect();
  const events: unknown[] = [];
  const listening = listen("sage:changed", (event) => events.push(event));
  const request = socket.sent.at(-1)!;
  expect(request.cmd).toBe("plugin:event|listen");
  const handlerId = request.args.handler as number;
  socket.receive({ id: request.id, ok: handlerId });
  const unlisten = await listening;
  socket.receive({
    event: "sage:changed",
    handlerId,
    payload: { revision: 2 },
  });
  expect(events).toEqual([
    { event: "sage:changed", id: handlerId, payload: { revision: 2 } },
  ]);
  const removed = unlisten();
  socket.receive({
    event: "sage:changed",
    handlerId,
    payload: { revision: 3 },
  });
  expect(events).toHaveLength(1);
  socket.receive({ id: socket.sent.at(-1)!.id, ok: null });
  await removed;
});

it("delivers an event emitted while the listener acknowledgement is still pending", async () => {
  const socket = await connect();
  const events: unknown[] = [];
  const listening = listen("sage:changed", (event) => events.push(event));
  const request = socket.sent.at(-1)!;
  const handlerId = request.args.handler as number;
  socket.receive({ event: "sage:changed", handlerId, payload: "early" });
  expect(events).toEqual([
    { event: "sage:changed", id: handlerId, payload: "early" },
  ]);
  socket.receive({ id: request.id, ok: handlerId });
  await listening;
});

it("rejects outstanding invokes on disconnect and caps pending requests", async () => {
  const socket = await connect();
  const waiting = Array.from({ length: 64 }, () =>
    invoke("get_bootstrap_snapshot"),
  );
  const settled = Promise.allSettled(waiting);
  await expect(invoke("get_bootstrap_snapshot")).rejects.toThrow(
    "invoke limit",
  );
  socket.close();
  expect((await settled).every((result) => result.status === "rejected")).toBe(
    true,
  );
  await expect(invoke("get_bootstrap_snapshot")).rejects.toThrow(
    "disconnected",
  );
});

it("reloads with the tab session token instead of reusing the consumed pairing code", async () => {
  const socket = await connect();
  socket.close();
  const reloaded = installBrowserBridge();
  const next = FakeSocket.latest;
  next.open();
  expect(next.sent[0]).toEqual({ session: "b".repeat(64) });
  next.receive({ session: "b".repeat(64) });
  await reloaded;
});

it("releases callbacks from rejected API listeners so subscriptions recover after saturation", async () => {
  const socket = await connect();
  const waiting = Array.from({ length: 64 }, () =>
    invoke("get_bootstrap_snapshot"),
  );
  for (let index = 0; index < 256; index++)
    await expect(listen("sage:changed", () => {})).rejects.toThrow(
      "invoke limit",
    );
  for (const request of socket.sent.slice(-64))
    socket.receive({ id: request.id, ok: null });
  await Promise.all(waiting);
  const listening = listen("sage:changed", () => {});
  const recovered = socket.sent.at(-1)!;
  socket.receive({ id: recovered.id, ok: recovered.args.handler });
  await expect(listening).resolves.toBeTypeOf("function");
});

it("resolves a binary invoke response as an ArrayBuffer, as the app's IPC does", async () => {
  const socket = await connect();
  expect(socket.binaryType).toBe("arraybuffer");
  const thumbnail = invoke("read_chat_image_thumbnail", { imageId: "one" });
  const { id } = socket.sent.at(-1)!;
  const frame = new Uint8Array([0, 0, 0, id, 0x89, 0x50, 0x4e, 0x47]);
  socket.receiveBinary(frame.buffer);
  const bytes = await thumbnail;
  expect(bytes).toBeInstanceOf(ArrayBuffer);
  expect([...new Uint8Array(bytes as ArrayBuffer)]).toEqual([
    0x89, 0x50, 0x4e, 0x47,
  ]);
});

it("retries a session reconnect refused while the previous page still holds the session", async () => {
  sessionStorage.setItem(
    "sage-browser-bridge:session",
    JSON.stringify({ port: "32123", session: "b".repeat(64) }),
  );
  history.replaceState(null, "", "/?bridge=32123");
  vi.useFakeTimers();
  const reloaded = installBrowserBridge();
  const refused = FakeSocket.latest;
  refused.close();
  await vi.advanceTimersByTimeAsync(400);
  const retry = FakeSocket.latest;
  expect(retry).not.toBe(refused);
  retry.open();
  expect(retry.sent[0]).toEqual({ session: "b".repeat(64) });
  retry.receive({ session: "b".repeat(64) });
  await reloaded;
  expect(document.querySelector('[role="alert"]')).toBeNull();
});

it("never retries a refused pairing code and tells the user when a live session drops", async () => {
  const refused = installBrowserBridge();
  FakeSocket.latest.close();
  await expect(refused).rejects.toThrow("disconnected");

  history.replaceState(null, "", "/?bridge=32123#" + "a".repeat(64));
  const socket = await connect();
  socket.close();
  expect(document.querySelector('[role="alert"]')?.textContent).toContain(
    "Reload this tab",
  );
});

import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act, cleanup, render, waitFor } from "@testing-library/react";
import { createElement } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { isTauriRuntime } from "../ports/nativeInvoke";
import { MAX_IMAGE_UPLOAD_BYTES } from "../domain/attachedContext";
import { installBrowserBridge } from "./installBrowserBridge";
import { MAX_MESSAGE_BYTES } from "./limits";

class FakeSocket extends EventTarget {
  static readonly OPEN = 1;
  static latest: FakeSocket;
  readyState = 0;
  bufferedAmount = 0;
  binaryType = "blob";
  sent: { id: number; cmd: string; args: Record<string, unknown> }[] = [];
  onRequest?: (request: FakeSocket["sent"][number]) => void;
  constructor(readonly url: string) {
    super();
    FakeSocket.latest = this;
  }
  send(data: string) {
    const request = JSON.parse(data);
    this.sent.push(request);
    this.onRequest?.(request);
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
  cleanup();
  FakeSocket.latest?.close();
  document.body.replaceChildren();
  vi.useRealTimers();
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  Reflect.deleteProperty(window, "__TAURI_EVENT_PLUGIN_INTERNALS__");
  vi.unstubAllGlobals();
});

it("flushes browser edits and settings before the shared workbench close persists layout and destroys the host", async () => {
  const socket = await connect();
  const { resetWorkbenchPortsForTests } =
    await import("../ports/workbenchRuntime");
  resetWorkbenchPortsForTests();
  const { App } = await import("../app/App");
  const { useWorkbenchStore } = await import("../app/store");
  const { useEditorSettingsStore } = await import("../app/editorSettingsStore");
  const { getFileEditorController } =
    await import("../features/files/fileEditorController");
  const initial = useWorkbenchStore.getState();
  const resource = { rootId: "root-a", relativePath: "draft.txt" };
  const version = { fingerprint: "base", byteLen: 0, modifiedAt: null };
  let editorWrite: FakeSocket["sent"][number] | undefined;
  let settingsWrite: FakeSocket["sent"][number] | undefined;
  let closeHandler: number | undefined;
  let destroyed = false;
  let persistedLayout: unknown;
  socket.onRequest = (request) => {
    if (request.cmd === "snapshot_editor_buffer") {
      editorWrite = request;
      return;
    }
    if (request.cmd === "set_editor_settings") {
      settingsWrite = request;
      return;
    }
    if (request.cmd === "persist_project_layout_and_close") {
      persistedLayout = request.args.request;
      destroyed = true;
    }
    if (request.args.event === "tauri://close-requested")
      closeHandler = request.args.handler as number;
    socket.receive({
      id: request.id,
      ok:
        request.cmd === "plugin:event|listen"
          ? request.args.handler
          : request.cmd === "list_harnesses"
            ? []
            : null,
    });
  };
  useWorkbenchStore.setState({
    phase: "booting",
    initialize: async () => {},
    refreshRepository: async () => {},
    project: { id: "project-a", name: "A", roots: [] },
    tabs: [],
    activeEditor: getFileEditorController().ready({
      resource,
      disk: version,
      base: version,
      buffer: { ...version, revision: 0, content: "" },
      lineEnding: "none",
      dirty: false,
      conflict: null,
    }),
  });
  useWorkbenchStore.getState().updateEditorContent("unsaved edit");
  useEditorSettingsStore.setState({ persistenceReady: true });
  useEditorSettingsStore.getState().setSettings({ "editor.fontSize": 16 });
  const screen = render(createElement(App));
  try {
    await waitFor(() => expect(closeHandler).toBeTypeOf("number"));
    act(() =>
      socket.receive({
        event: "tauri://close-requested",
        handlerId: closeHandler,
        payload: null,
      }),
    );
    await waitFor(() => expect(editorWrite).toBeDefined());
    expect(editorWrite!.args.request).toMatchObject({
      content: "unsaved edit",
      revision: 1,
    });
    expect(destroyed).toBe(false);
    await act(async () => {
      socket.receive({
        id: editorWrite!.id,
        ok: { revision: 1, persistedAt: 1, dirty: true, applied: true },
      });
    });
    expect(useWorkbenchStore.getState().activeEditor).toMatchObject({
      persistedRevision: 1,
    });
    await waitFor(() => expect(settingsWrite).toBeDefined());
    expect(destroyed).toBe(false);
    expect(settingsWrite!.args.changes).toEqual({ "editor.fontSize": 16 });
    await act(async () =>
      socket.receive({ id: settingsWrite!.id, ok: { "editor.fontSize": 16 } }),
    );
    await waitFor(() => expect(destroyed).toBe(true));
    expect(persistedLayout).toMatchObject({ projectId: "project-a", tabs: [] });
    expect(screen.queryByRole("alert")).toBeNull();
  } finally {
    screen.unmount();
    useWorkbenchStore.setState(initial, true);
  }
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

it.each([false, true])(
  "guards an unflushed edit until snapshot and layout persistence complete (layout failure: %s)",
  async (failLayout) => {
    const socket = await connect();
    const { installWorkbenchNavigationGuard } =
      await import("../app/installWorkbenchNavigationGuard");
    const { resetWorkbenchPortsForTests } =
      await import("../ports/workbenchRuntime");
    resetWorkbenchPortsForTests();
    const { useWorkbenchStore } = await import("../app/store");
    const { getFileEditorController } =
      await import("../features/files/fileEditorController");
    const initial = useWorkbenchStore.getState();
    const resource = { rootId: "root-a", relativePath: "draft.txt" };
    const version = { fingerprint: "base", byteLen: 0, modifiedAt: null };
    let snapshot: FakeSocket["sent"][number] | undefined;
    let layout: FakeSocket["sent"][number] | undefined;
    socket.onRequest = (request) => {
      if (request.cmd === "snapshot_editor_buffer") snapshot = request;
      else if (request.cmd === "persist_project_layout") layout = request;
      else socket.receive({ id: request.id, ok: null });
    };
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      projectTransitioning: false,
      tabs: [],
      activeEditor: getFileEditorController().ready({
        resource,
        disk: version,
        base: version,
        buffer: { ...version, revision: 0, content: "" },
        lineEnding: "none",
        dirty: false,
        conflict: null,
      }),
    });
    const dispose = installWorkbenchNavigationGuard();
    const navigate = () => {
      const event = new Event("beforeunload", { cancelable: true });
      window.dispatchEvent(event);
      return event.defaultPrevented;
    };
    try {
      expect(navigate()).toBe(false);
      expect(snapshot).toBeUndefined();
      expect(layout).toBeUndefined();
      useWorkbenchStore.getState().updateEditorContent("unsaved edit");
      expect(navigate()).toBe(true);
      await waitFor(() => expect(snapshot).toBeDefined());
      expect(snapshot!.args.request).toMatchObject({
        resource,
        content: "unsaved edit",
        revision: 1,
      });
      socket.receive({
        id: snapshot!.id,
        ok: { revision: 1, persistedAt: 1, dirty: true, applied: true },
      });
      await waitFor(() => expect(layout).toBeDefined());
      expect(layout!.args.request).toMatchObject({ projectId: "project-a" });
      expect(useWorkbenchStore.getState().activeEditor).toMatchObject({
        persistedRevision: 1,
      });
      expect(navigate()).toBe(true);
      if (failLayout) {
        const failed = layout!;
        socket.receive({ id: failed.id, err: "layout write failed" });
        await waitFor(() =>
          expect(useWorkbenchStore.getState().error).toBe(
            "layout write failed",
          ),
        );
        expect(navigate()).toBe(true);
        await waitFor(() => expect(layout).not.toBe(failed));
      }
      socket.receive({ id: layout!.id, ok: null });
      await waitFor(() => expect(navigate()).toBe(false));
    } finally {
      dispose();
      useWorkbenchStore.setState(initial, true);
    }
  },
);

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

it("transports the largest composer image upload including escaped metadata", async () => {
  const socket = await connect();
  const request = {
    projectId: "\0".repeat(512),
    projectGeneration: Number.MAX_SAFE_INTEGER,
    chatId: "\0".repeat(512),
    mediaType: "image/jpeg",
    dataBase64: btoa("\0".repeat(MAX_IMAGE_UPLOAD_BYTES)),
    label: "\0".repeat(80),
  };
  const uploaded = invoke("upload_chat_image", { request });
  const received = socket.sent.at(-1)!;
  expect(received.cmd).toBe("upload_chat_image");
  expect(received.args).toEqual({ request });
  socket.receive({ id: received.id, ok: { artifactId: "uploaded-image" } });
  await expect(uploaded).resolves.toEqual({ artifactId: "uploaded-image" });
});

it("admits exactly the message byte bound and refuses one byte more in either direction", async () => {
  const socket = await connect();
  const envelope = JSON.stringify({
    id: 1,
    cmd: "get_bootstrap_snapshot",
    args: { padding: "" },
  });
  const padding = "a".repeat(MAX_MESSAGE_BYTES - envelope.length);
  const accepted = invoke("get_bootstrap_snapshot", { padding });
  const request = socket.sent.at(-1)!;
  expect(new Blob([JSON.stringify(request)]).size).toBe(MAX_MESSAGE_BYTES);
  socket.receive({ id: request.id, ok: "accepted" });
  await expect(accepted).resolves.toBe("accepted");
  await expect(
    invoke("get_bootstrap_snapshot", { padding: padding + "a" }),
  ).rejects.toThrow("message limit");

  const received: number[] = [];
  const listening = listen<string>("sage:changed", (event) =>
    received.push(event.payload.length),
  );
  const subscription = socket.sent.at(-1)!;
  const handlerId = subscription.args.handler as number;
  socket.receive({ id: subscription.id, ok: handlerId });
  await listening;
  const frame = { event: "sage:changed", handlerId, payload: "" };
  frame.payload = "a".repeat(MAX_MESSAGE_BYTES - JSON.stringify(frame).length);
  socket.receive(frame);
  expect(received).toEqual([frame.payload.length]);
  const waiting = invoke("get_bootstrap_snapshot");
  const disconnected = expect(waiting).rejects.toThrow("disconnected");
  socket.receive({ ...frame, payload: frame.payload + "a" });
  await disconnected;
  expect(received).toHaveLength(1);
});

it("includes bytes already buffered on the socket in the send budget", async () => {
  const socket = await connect();
  const frameBytes = JSON.stringify({
    id: 1,
    cmd: "get_bootstrap_snapshot",
    args: {},
  }).length;
  socket.bufferedAmount = MAX_MESSAGE_BYTES - frameBytes + 1;
  await expect(invoke("get_bootstrap_snapshot")).rejects.toThrow(
    "message limit",
  );
  socket.bufferedAmount--;
  const accepted = invoke("get_bootstrap_snapshot");
  socket.receive({ id: socket.sent.at(-1)!.id, ok: "within budget" });
  await expect(accepted).resolves.toBe("within budget");
});

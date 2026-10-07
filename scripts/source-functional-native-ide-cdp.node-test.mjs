import assert from "node:assert/strict";
import test from "node:test";

import { CdpError, connectCdp } from "./source-functional-native-ide-cdp.mjs";

class FakeWebSocket {
  static instances = [];

  constructor(url) {
    this.url = url;
    this.listeners = new Map();
    this.sent = [];
    this.autoClose = true;
    FakeWebSocket.instances.push(this);
  }

  addEventListener(name, callback) {
    const values = this.listeners.get(name) ?? [];
    values.push(callback);
    this.listeners.set(name, values);
  }

  emit(name, value = {}) {
    for (const callback of this.listeners.get(name) ?? []) callback(value);
  }

  send(value) {
    this.sent.push(JSON.parse(value));
  }

  close() {
    if (this.autoClose)
      queueMicrotask(() => this.emit("close", { code: 1000 }));
  }
}

function target(overrides = {}) {
  return {
    id: "page id/α",
    type: "page",
    title: "Sage",
    url: "http://127.0.0.1:1420/",
    webSocketDebuggerUrl:
      "ws://127.0.0.1:9229/devtools/page/page%20id%2F%CE%B1",
    ...overrides,
  };
}

async function client(overrides = {}) {
  FakeWebSocket.instances = [];
  const promise = connectCdp({
    target: target(),
    deadline: Date.now() + 1_000,
    WebSocketImpl: FakeWebSocket,
    ...overrides,
  });
  const socket = FakeWebSocket.instances[0];
  socket.emit("open");
  return { client: await promise, socket };
}

function respond(socket, id, result) {
  socket.emit("message", { data: JSON.stringify({ id, result }) });
}

test("uses exactly one canonical page WebSocket and serial monotonic commands", async () => {
  const { client: cdp, socket } = await client();
  assert.equal(
    socket.url,
    "ws://127.0.0.1:9229/devtools/page/page%20id%2F%CE%B1",
  );
  const first = cdp.evaluate("true", Date.now() + 1_000);
  assert.deepEqual(socket.sent[0], {
    id: 1,
    method: "Runtime.evaluate",
    params: {
      expression: "true",
      awaitPromise: true,
      returnByValue: true,
      userGesture: true,
    },
  });
  await assert.rejects(
    cdp.evaluate("false", Date.now() + 1_000),
    /journey-cdp-command-in-flight/u,
  );
  respond(socket, 1, { result: { type: "boolean", value: true } });
  assert.equal(await first, true);
  const second = cdp.evaluate("2", Date.now() + 1_000);
  assert.equal(socket.sent[1].id, 2);
  respond(socket, 2, { result: { type: "number", value: 2 } });
  assert.equal(await second, 2);
  assert.deepEqual(cdp.stats, {
    connectionCount: 1,
    maxInFlight: 1,
    requestCount: 2,
  });
  assert.equal(Object.isFrozen(cdp.stats), true);
  await cdp.close(Date.now() + 1_000);
  assert.equal(FakeWebSocket.instances.length, 1);
});

test("open deadline bounds only startup while the lifetime deadline caps commands", async () => {
  FakeWebSocket.instances = [];
  const unopened = connectCdp({
    target: target(),
    deadline: Date.now() + 1_000,
    openDeadline: Date.now() + 12,
    WebSocketImpl: FakeWebSocket,
  }).then(
    () => "opened",
    (error) => error.message,
  );
  assert.equal(
    await Promise.race([
      unopened,
      new Promise((resolve) => setTimeout(() => resolve("still-waiting"), 50)),
    ]),
    "journey-cdp-deadline",
  );

  FakeWebSocket.instances = [];
  const lifetimeDeadline = Date.now() + 1_000;
  const promise = connectCdp({
    target: target(),
    deadline: lifetimeDeadline,
    openDeadline: Date.now() + 12,
    WebSocketImpl: FakeWebSocket,
  });
  const socket = FakeWebSocket.instances[0];
  socket.emit("open");
  const cdp = await promise;
  await new Promise((resolve) => setTimeout(resolve, 20));
  const result = cdp.evaluate("true", Date.now() + 500);
  assert.equal(socket.sent.length, 1);
  respond(socket, 1, { result: { type: "boolean", value: true } });
  assert.equal(await result, true);
  await cdp.close(Date.now() + 1_000);

  FakeWebSocket.instances = [];
  await assert.rejects(
    connectCdp({
      target: target(),
      deadline: Date.now() + 12,
      openDeadline: Date.now() + 1_000,
      WebSocketImpl: FakeWebSocket,
    }),
    /journey-cdp-deadline/u,
  );
});

test("tolerates ambient discovery keys and accepts a legal remote description", async () => {
  const { client: cdp, socket } = await client({
    target: target({ browserContextId: "ambient-from-json" }),
  });
  const value = cdp.evaluate("'Sage'", Date.now() + 1_000);
  respond(socket, 1, {
    result: { type: "string", value: "Sage", description: "Sage" },
  });
  assert.equal(await value, "Sage");
  const mismatched = cdp.evaluate("true", Date.now() + 1_000);
  respond(socket, 2, { result: { type: "string", value: true } });
  await assert.rejects(mismatched, /journey-cdp-evaluate-invalid/u);
  await cdp.close(Date.now() + 1_000);
});

test("accepts Chromium's nullable primitive subtype metadata", async () => {
  const { client: cdp, socket } = await client();
  const value = cdp.evaluate("true", Date.now() + 1_000);
  respond(socket, 1, {
    result: { type: "boolean", subtype: null, value: true },
  });
  assert.equal(await value, true);
  await cdp.close(Date.now() + 1_000);
});

test("accepts Chromium's null exception-details sentinel for a successful evaluation", async () => {
  const { client: cdp, socket } = await client();
  const value = cdp.evaluate("true", Date.now() + 1_000);
  respond(socket, 1, {
    result: { type: "boolean", value: true },
    exceptionDetails: null,
  });
  assert.equal(await value, true);
  await cdp.close(Date.now() + 1_000);
});

test("captures only canonical bounded PNG base64", async () => {
  const { client: cdp, socket } = await client();
  const shot = cdp.screenshot(Date.now() + 1_000);
  assert.deepEqual(socket.sent[0], {
    id: 1,
    method: "Page.captureScreenshot",
    params: { format: "png", fromSurface: true },
  });
  const png = Buffer.concat([
    Buffer.from("89504e470d0a1a0a", "hex"),
    Buffer.from("x"),
  ]).toString("base64");
  respond(socket, 1, { data: png });
  assert.equal(await shot, png);
  await cdp.close(Date.now() + 1_000);
});

test("rejects invalid targets, values, frames, responses, and native errors", async () => {
  await assert.rejects(
    connectCdp({
      target: target({
        webSocketDebuggerUrl:
          "ws://localhost:9229/devtools/page/page%20id%2F%CE%B1",
      }),
      deadline: Date.now() + 1_000,
    }),
    /journey-cdp-target-invalid/u,
  );
  const { client: cdp, socket } = await client();
  const first = cdp.evaluate("({})", Date.now() + 1_000);
  respond(socket, 1, {
    result: { result: { type: "object", objectId: "forbidden" } },
  });
  await assert.rejects(first, /journey-cdp-evaluate-invalid/u);
  const second = cdp.evaluate("1", Date.now() + 1_000);
  socket.emit("message", { data: "{" });
  await assert.rejects(second, /journey-cdp-frame-invalid/u);
  await assert.rejects(
    cdp.evaluate("1", Date.now() + 1_000),
    /journey-cdp-frame-invalid/u,
  );
  const { client: oversizedCdp, socket: oversizedSocket } = await client();
  const oversizedPending = oversizedCdp.evaluate("1", Date.now() + 1_000);
  oversizedSocket.emit("message", {
    data: JSON.stringify({
      method: "Runtime.consoleAPICalled",
      params: { text: "x".repeat(12 * 1024 * 1024) },
    }),
  });
  await assert.rejects(oversizedPending, /journey-cdp-frame-invalid/u);
  const { client: errorCdp, socket: errorSocket } = await client();
  const command = errorCdp.evaluate("1", Date.now() + 1_000);
  errorSocket.emit("message", {
    data: JSON.stringify({ id: 1, error: { code: -1 } }),
  });
  await assert.rejects(command, /journey-cdp-command-failed/u);
  await errorCdp.close(Date.now() + 1_000);
});

test("rejects exception details and only admits canonical bounded PNG screenshots", async () => {
  const { client: cdp, socket } = await client();
  const exceptional = cdp.evaluate("throw new Error()", Date.now() + 1_000);
  respond(socket, 1, {
    result: {
      result: { type: "object", subtype: "error", value: {} },
      exceptionDetails: { text: "boom" },
    },
  });
  await assert.rejects(exceptional, /journey-cdp-evaluate-invalid/u);

  const malformed = cdp.screenshot(Date.now() + 1_000);
  respond(socket, 2, { data: "not-base64" });
  await assert.rejects(malformed, /journey-cdp-screenshot-invalid/u);

  const notPng = cdp.screenshot(Date.now() + 1_000);
  respond(socket, 3, { data: Buffer.from("not png").toString("base64") });
  await assert.rejects(notPng, /journey-cdp-screenshot-invalid/u);

  const tooLarge = cdp.screenshot(Date.now() + 1_000);
  const oversized = Buffer.concat([
    Buffer.from("89504e470d0a1a0a", "hex"),
    Buffer.alloc(8 * 1024 * 1024),
  ]).toString("base64");
  respond(socket, 4, { data: oversized });
  await assert.rejects(tooLarge, /journey-cdp-screenshot-invalid/u);
  await cdp.close(Date.now() + 1_000);
});

test("treats duplicate or unknown responses and notification floods as terminal", async () => {
  const { client: cdp, socket } = await client();
  const request = cdp.evaluate("1", Date.now() + 1_000);
  respond(socket, 2, { result: { type: "number", value: 2 } });
  await assert.rejects(request, /journey-cdp-response-unexpected/u);
  const { client: floodCdp, socket: floodSocket } = await client();
  const pending = floodCdp.evaluate("1", Date.now() + 1_000);
  for (let index = 0; index <= 1024; index += 1)
    floodSocket.emit("message", {
      data: JSON.stringify({
        method: "Runtime.consoleAPICalled",
        params: { index },
      }),
    });
  await assert.rejects(pending, /journey-cdp-notifications-exceeded/u);

  const { client: byteCdp, socket: byteSocket } = await client();
  const bytePending = byteCdp.evaluate("1", Date.now() + 1_000);
  const largeNotification = JSON.stringify({
    method: "Runtime.consoleAPICalled",
    params: { text: "x".repeat(1024 * 1024) },
  });
  for (let index = 0; index < 5; index += 1)
    byteSocket.emit("message", { data: largeNotification });
  await assert.rejects(bytePending, /journey-cdp-notifications-exceeded/u);
});

test("bounds open, requests, and close and leaves no pending request after close", async () => {
  FakeWebSocket.instances = [];
  await assert.rejects(
    connectCdp({
      target: target(),
      deadline: Date.now() + 12,
      WebSocketImpl: FakeWebSocket,
    }),
    /journey-cdp-deadline/u,
  );
  const { client: cdp, socket } = await client();
  await assert.rejects(
    cdp.evaluate("1", Date.now() - 1),
    /journey-cdp-deadline/u,
  );
  const pending = cdp.evaluate("1", Date.now() + 1_000);
  await cdp.close(Date.now() + 1_000);
  await assert.rejects(pending, /journey-cdp-closed/u);
  assert.equal(cdp.stats.maxInFlight, 1);
  assert.equal(cdp.stats.requestCount, 1);
  socket.emit("message", {
    data: JSON.stringify({
      id: 1,
      result: { result: { type: "number", value: 1 } },
    }),
  });

  const { client: timeoutCdp } = await client();
  await assert.rejects(
    timeoutCdp.evaluate("1", Date.now() + 12),
    /journey-cdp-deadline/u,
  );
  const { client: expiredCdp, socket: expiredSocket } = await client({
    deadline: Date.now() + 16,
  });
  await new Promise((resolve) => setTimeout(resolve, 24));
  await assert.rejects(
    expiredCdp.evaluate("1", Date.now() + 1_000),
    /journey-cdp-deadline/u,
  );
  assert.equal(expiredSocket.sent.length, 0);
  const cleanup = expiredCdp.close(Date.now() + 5_000);
  assert.equal(cleanup, expiredCdp.close(Date.now() + 5_000));
  await cleanup;
  assert.equal(expiredSocket.sent.length, 0);
  const { client: closingCdp, socket: closingSocket } = await client();
  closingSocket.autoClose = false;
  const firstClose = closingCdp.close(Date.now() + 12);
  const secondClose = closingCdp.close(Date.now() + 12);
  assert.equal(firstClose, secondClose);
  await assert.rejects(firstClose, /journey-cdp-close-deadline/u);
});

test("reports close and socket errors", async () => {
  const { client: cdp, socket } = await client();
  const pending = cdp.evaluate("1", Date.now() + 1_000);
  socket.emit("error");
  await assert.rejects(pending, CdpError);
  await assert.rejects(
    cdp.evaluate("1", Date.now() + 1_000),
    /journey-cdp-websocket-error/u,
  );

  const { client: closeCdp, socket: closeSocket } = await client();
  const closePending = closeCdp.evaluate("1", Date.now() + 1_000);
  closeSocket.emit("close", { code: 1006 });
  await assert.rejects(closePending, /journey-cdp-websocket-closed/u);
});

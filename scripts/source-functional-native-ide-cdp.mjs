const MAX_TARGET_ID_BYTES = 256;
const MAX_EXPRESSION_BYTES = 64 * 1024;
const MAX_FRAME_BYTES = 12 * 1024 * 1024;
const MAX_EVALUATE_BYTES = 256 * 1024;
const MAX_SCREENSHOT_BYTES = 8 * 1024 * 1024;
const MAX_NOTIFICATION_COUNT = 1024;
const MAX_NOTIFICATION_BYTES = 4 * 1024 * 1024;
const MAX_VALUE_DEPTH = 32;
const PNG_SIGNATURE = Buffer.from("89504e470d0a1a0a", "hex");

export class CdpError extends Error {
  constructor(code) {
    super(`journey-cdp-${code}`);
    this.name = "CdpError";
  }
}

function fail(code) {
  return new CdpError(code);
}

function exactKeys(value, keys) {
  return (
    value &&
    typeof value === "object" &&
    !Array.isArray(value) &&
    Object.keys(value).sort().join("\0") === [...keys].sort().join("\0")
  );
}

function deadlineAt(deadline) {
  if (!Number.isSafeInteger(deadline) || deadline <= Date.now())
    throw fail("deadline");
  return deadline;
}

function effectiveDeadline(operationDeadline, connectionDeadline) {
  if (!Number.isSafeInteger(operationDeadline)) throw fail("deadline");
  return deadlineAt(Math.min(operationDeadline, connectionDeadline));
}

function timeoutAt(deadline, callback) {
  return setTimeout(callback, Math.max(0, deadline - Date.now()));
}

function bytes(value) {
  return Buffer.byteLength(value, "utf8");
}

function canonicalWebSocketUrl(target) {
  if (
    !target ||
    typeof target !== "object" ||
    Array.isArray(target) ||
    target.type !== "page" ||
    typeof target.id !== "string" ||
    Buffer.byteLength(target.id, "utf8") < 1 ||
    Buffer.byteLength(target.id, "utf8") > MAX_TARGET_ID_BYTES ||
    typeof target.title !== "string" ||
    typeof target.url !== "string" ||
    typeof target.webSocketDebuggerUrl !== "string"
  )
    throw fail("target-invalid");
  let parsed;
  try {
    parsed = new URL(target.webSocketDebuggerUrl);
  } catch {
    throw fail("target-invalid");
  }
  if (
    parsed.protocol !== "ws:" ||
    parsed.hostname !== "127.0.0.1" ||
    parsed.username ||
    parsed.password ||
    parsed.search ||
    parsed.hash ||
    !/^\d{4,5}$/u.test(parsed.port) ||
    Number(parsed.port) < 1024 ||
    Number(parsed.port) > 65535
  )
    throw fail("target-invalid");
  const canonical = `ws://127.0.0.1:${parsed.port}/devtools/page/${encodeURIComponent(target.id)}`;
  if (target.webSocketDebuggerUrl !== canonical) throw fail("target-invalid");
  return canonical;
}

function isJsonValue(value, depth = 0) {
  if (depth > MAX_VALUE_DEPTH) return false;
  if (
    value === null ||
    typeof value === "boolean" ||
    typeof value === "string" ||
    (typeof value === "number" && Number.isFinite(value))
  )
    return true;
  if (Array.isArray(value))
    return value.every((item) => isJsonValue(item, depth + 1));
  if (Object.getPrototypeOf(value) !== Object.prototype) return false;
  return Object.values(value).every((item) => isJsonValue(item, depth + 1));
}

function canonicalBase64Png(value) {
  if (typeof value !== "string" || value.length === 0 || value.length % 4 !== 0)
    throw fail("screenshot-invalid");
  if (!/^[A-Za-z0-9+/]*={0,2}$/u.test(value)) throw fail("screenshot-invalid");
  const decoded = Buffer.from(value, "base64");
  if (
    decoded.length < PNG_SIGNATURE.length ||
    decoded.length > MAX_SCREENSHOT_BYTES ||
    !decoded.subarray(0, PNG_SIGNATURE.length).equals(PNG_SIGNATURE) ||
    decoded.toString("base64") !== value
  )
    throw fail("screenshot-invalid");
  return value;
}

function remoteValue(result) {
  if (
    !result ||
    typeof result !== "object" ||
    Array.isArray(result) ||
    !Object.hasOwn(result, "result") ||
    (result.exceptionDetails !== undefined &&
      result.exceptionDetails !== null) ||
    Object.keys(result).some(
      (key) => key !== "result" && key !== "exceptionDetails",
    )
  )
    throw fail("evaluate-invalid");
  const remote = result.result;
  if (
    !remote ||
    typeof remote !== "object" ||
    Array.isArray(remote) ||
    remote.objectId !== undefined ||
    !Object.hasOwn(remote, "value") ||
    typeof remote.type !== "string" ||
    Object.keys(remote).some(
      (key) => !["type", "subtype", "value", "description"].includes(key),
    ) ||
    (remote.subtype !== undefined &&
      remote.subtype !== null &&
      typeof remote.subtype !== "string") ||
    (remote.description !== undefined &&
      (typeof remote.description !== "string" ||
        bytes(remote.description) > MAX_EVALUATE_BYTES)) ||
    !isJsonValue(remote.value) ||
    bytes(JSON.stringify(remote.value)) > MAX_EVALUATE_BYTES
  )
    throw fail("evaluate-invalid");
  const expectedType =
    remote.value === null ||
    Array.isArray(remote.value) ||
    typeof remote.value === "object"
      ? "object"
      : typeof remote.value;
  if (remote.type !== expectedType) throw fail("evaluate-invalid");
  return remote.value;
}

function screenshotValue(result) {
  if (!exactKeys(result, ["data"])) throw fail("screenshot-invalid");
  return canonicalBase64Png(result.data);
}

/**
 * Open one raw, page-scoped CDP connection. `deadline` and every method
 * deadline are absolute Unix milliseconds.
 */
export async function connectCdp({
  target,
  deadline,
  openDeadline,
  WebSocketImpl,
} = {}) {
  const connectionDeadline = deadlineAt(deadline);
  const socketOpenDeadline =
    openDeadline === undefined
      ? connectionDeadline
      : effectiveDeadline(openDeadline, connectionDeadline);
  const url = canonicalWebSocketUrl(target);
  const Socket = WebSocketImpl ?? globalThis.WebSocket;
  if (typeof Socket !== "function") throw fail("websocket-unavailable");

  let socket;
  try {
    socket = new Socket(url);
  } catch {
    throw fail("websocket-open");
  }
  if (!socket || typeof socket.addEventListener !== "function")
    throw fail("websocket-open");

  let terminal = null;
  let opened = false;
  let closed = false;
  let pending = null;
  let closeWaiter = null;
  let nextId = 0;
  let requestCount = 0;
  let maxInFlight = 0;
  let notificationCount = 0;
  let notificationBytes = 0;
  const stats = Object.freeze(
    Object.defineProperties(
      {},
      {
        connectionCount: { enumerable: true, get: () => 1 },
        maxInFlight: { enumerable: true, get: () => maxInFlight },
        requestCount: { enumerable: true, get: () => requestCount },
      },
    ),
  );

  const closeSocket = () => {
    try {
      socket.close();
    } catch {
      // A terminal condition already records the cause.
    }
  };
  const terminate = (error) => {
    if (terminal) return;
    terminal = error;
    if (pending) {
      const waiter = pending;
      pending = null;
      clearTimeout(waiter.timer);
      waiter.reject(error);
    }
    if (closeWaiter) {
      const waiter = closeWaiter;
      closeWaiter = null;
      clearTimeout(waiter.timer);
      waiter.reject(error);
    }
    closeSocket();
  };
  socket.addEventListener("message", ({ data }) => {
    if (closed) return;
    if (typeof data !== "string" || bytes(data) > MAX_FRAME_BYTES) {
      terminate(fail("frame-invalid"));
      return;
    }
    let message;
    try {
      message = JSON.parse(data);
    } catch {
      terminate(fail("frame-invalid"));
      return;
    }
    if (!message || typeof message !== "object" || Array.isArray(message)) {
      terminate(fail("frame-invalid"));
      return;
    }
    if (!Object.hasOwn(message, "id")) {
      notificationCount += 1;
      notificationBytes += bytes(data);
      if (
        notificationCount > MAX_NOTIFICATION_COUNT ||
        notificationBytes > MAX_NOTIFICATION_BYTES
      )
        terminate(fail("notifications-exceeded"));
      return;
    }
    if (
      !pending ||
      !Number.isSafeInteger(message.id) ||
      message.id !== pending.id
    ) {
      terminate(fail("response-unexpected"));
      return;
    }
    const waiter = pending;
    pending = null;
    clearTimeout(waiter.timer);
    if (exactKeys(message, ["id", "error"])) {
      waiter.reject(fail("command-failed"));
      return;
    }
    if (!exactKeys(message, ["id", "result"])) {
      terminate(fail("response-invalid"));
      return;
    }
    waiter.resolve(message.result);
  });
  socket.addEventListener("error", () => terminate(fail("websocket-error")));
  socket.addEventListener("close", () => {
    if (closed) return;
    closed = true;
    if (closeWaiter) {
      const waiter = closeWaiter;
      closeWaiter = null;
      clearTimeout(waiter.timer);
      terminal = terminal ?? fail("closed");
      waiter.resolve();
      return;
    }
    terminate(fail(opened ? "websocket-closed" : "websocket-open"));
  });

  await new Promise((resolve, reject) => {
    const timer = timeoutAt(socketOpenDeadline, () => {
      const error = fail("deadline");
      terminate(error);
      reject(error);
    });
    socket.addEventListener("open", () => {
      if (terminal || closed) return;
      clearTimeout(timer);
      opened = true;
      resolve();
    });
    socket.addEventListener("error", () => {
      clearTimeout(timer);
      reject(fail("websocket-open"));
    });
    socket.addEventListener("close", () => {
      clearTimeout(timer);
      reject(fail("websocket-open"));
    });
  });

  const command = (method, params, operationDeadline) => {
    if (terminal || closed || closeWaiter)
      return Promise.reject(terminal ?? fail("closed"));
    if (pending) return Promise.reject(fail("command-in-flight"));
    operationDeadline = effectiveDeadline(
      operationDeadline,
      connectionDeadline,
    );
    const id = ++nextId;
    if (!Number.isSafeInteger(id)) return Promise.reject(fail("id-exhausted"));
    requestCount += 1;
    return new Promise((resolve, reject) => {
      const timer = timeoutAt(operationDeadline, () =>
        terminate(fail("deadline")),
      );
      pending = { id, timer, resolve, reject };
      maxInFlight = Math.max(maxInFlight, 1);
      try {
        socket.send(JSON.stringify({ id, method, params }));
      } catch {
        terminate(fail("send"));
      }
    });
  };

  return Object.freeze({
    evaluate: async (expression, operationDeadline) => {
      if (
        typeof expression !== "string" ||
        expression.length === 0 ||
        bytes(expression) > MAX_EXPRESSION_BYTES
      )
        throw fail("expression-invalid");
      return remoteValue(
        await command(
          "Runtime.evaluate",
          {
            expression,
            awaitPromise: true,
            returnByValue: true,
            userGesture: true,
          },
          operationDeadline,
        ),
      );
    },
    screenshot: async (operationDeadline) =>
      screenshotValue(
        await command(
          "Page.captureScreenshot",
          { format: "png", fromSurface: true },
          operationDeadline,
        ),
      ),
    close: (operationDeadline) => {
      if (closeWaiter) return closeWaiter.promise;
      if (closed) return Promise.resolve();
      if (terminal) return Promise.reject(terminal);
      // Teardown is independently bounded: it must still be able to release
      // the socket after the action/connection deadline has elapsed.
      operationDeadline = deadlineAt(operationDeadline);
      if (pending) {
        const waiter = pending;
        pending = null;
        clearTimeout(waiter.timer);
        waiter.reject(fail("closed"));
      }
      let resolveClose;
      let rejectClose;
      const promise = new Promise((resolve, reject) => {
        resolveClose = resolve;
        rejectClose = reject;
      });
      const waiter = {
        promise,
        resolve: resolveClose,
        reject: rejectClose,
        timer: null,
      };
      closeWaiter = waiter;
      waiter.timer = timeoutAt(operationDeadline, () => {
        if (closeWaiter === waiter) {
          const error = fail("close-deadline");
          terminal = error;
          closeWaiter = null;
          closeSocket();
          rejectClose(error);
        }
      });
      closeSocket();
      return promise;
    },
    stats,
  });
}

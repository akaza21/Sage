import { MAX_MESSAGE_BYTES } from "./limits";

const MAX_PENDING = 64;
const MAX_CALLBACKS = 256;
const AUTH_TIMEOUT_MS = 10_000;
const RECONNECT_ATTEMPTS = 5;
const RECONNECT_DELAY_MS = 400;
const BRIDGE_NAME = "SAGE_DEV_BROWSER_BRIDGE";
const STORAGE_KEY = "sage-browser-bridge:session";

type Callback = { run: (value: unknown) => void; once: boolean };
type Pending = {
  resolve: (value: unknown) => void;
  reject: (reason: unknown) => void;
};
type Frame = {
  id?: number;
  ok?: unknown;
  err?: unknown;
  session?: string;
  event?: string;
  handlerId?: number;
  payload?: unknown;
};

function savedSession(port: string): string | undefined {
  const saved = sessionStorage.getItem(STORAGE_KEY);
  if (!saved) return undefined;
  try {
    const credential = JSON.parse(saved) as { port?: string; session?: string };
    return credential.port === port ? credential.session : undefined;
  } catch {
    sessionStorage.removeItem(STORAGE_KEY);
    return undefined;
  }
}

function showDisconnected() {
  const notice = document.createElement("div");
  notice.setAttribute("role", "alert");
  notice.textContent =
    "Sage browser bridge disconnected. Reload this tab to reconnect.";
  notice.style.cssText =
    "position: fixed; inset: auto 0 0; z-index: 2147483647; padding: 8px 16px; background: #5a1d1d; color: #fff; font: 13px system-ui";
  document.body.append(notice);
}

export async function installBrowserBridge(): Promise<void> {
  const pairingCode = location.hash.slice(1);
  history.replaceState(history.state, "", location.pathname + location.search);
  const port = new URLSearchParams(location.search).get("bridge") ?? "";
  if (!/^\d{1,5}$/.test(port) || Number(port) < 1 || Number(port) > 65535)
    throw new Error(`${BRIDGE_NAME}: invalid port`);
  const session = savedSession(port);
  if (!pairingCode && !session)
    throw new Error("Open the pairing link from Sage");
  const pending = new Map<number, Pending>();
  const callbacks = new Map<number, Callback>();
  let socket: WebSocket | undefined;
  let sequence = 0;
  let ready = false;

  const settle = (frame: Frame) => {
    if (
      typeof frame.event === "string" &&
      typeof frame.handlerId === "number"
    ) {
      const callback = callbacks.get(frame.handlerId);
      if (!callback) return;
      if (callback.once) callbacks.delete(frame.handlerId);
      callback.run({
        event: frame.event,
        id: frame.handlerId,
        payload: frame.payload,
      });
      return;
    }
    if (typeof frame.id !== "number") return;
    const request = pending.get(frame.id);
    if (!request) return;
    pending.delete(frame.id);
    if (Object.hasOwn(frame, "err")) request.reject(frame.err);
    else request.resolve(frame.ok);
  };

  const settleBinary = (data: ArrayBuffer) => {
    if (data.byteLength < 4) return;
    const id = new DataView(data).getUint32(0);
    const request = pending.get(id);
    if (!request) return;
    pending.delete(id);
    request.resolve(data.slice(4));
  };

  const pair = () =>
    new Promise<void>((resolve, reject) => {
      const current = new WebSocket(`ws://127.0.0.1:${port}`);
      current.binaryType = "arraybuffer";
      socket = current;
      const timer = setTimeout(() => {
        reject(new Error("Sage browser bridge pairing timed out"));
        current.close();
      }, AUTH_TIMEOUT_MS);
      const disconnected = () => {
        clearTimeout(timer);
        reject(new Error("Sage browser bridge disconnected"));
        if (!ready || socket !== current) return;
        ready = false;
        const error = new Error("Sage browser bridge disconnected");
        for (const request of pending.values()) request.reject(error);
        pending.clear();
        callbacks.clear();
        showDisconnected();
      };
      current.addEventListener("close", disconnected);
      current.addEventListener("error", () => {
        disconnected();
        current.close();
      });
      current.addEventListener(
        "open",
        () => {
          // Credentials go only in the first frame, never in a URL or an invoke.
          current.send(
            JSON.stringify(pairingCode ? { pairingCode } : { session }),
          );
        },
        { once: true },
      );
      current.addEventListener("message", (event: MessageEvent) => {
        if (event.data instanceof ArrayBuffer) {
          if (ready && event.data.byteLength <= MAX_MESSAGE_BYTES)
            settleBinary(event.data);
          else current.close();
          return;
        }
        if (
          typeof event.data !== "string" ||
          new Blob([event.data]).size > MAX_MESSAGE_BYTES
        ) {
          current.close();
          return;
        }
        let frame: Frame;
        try {
          frame = JSON.parse(event.data) as Frame;
        } catch {
          current.close();
          return;
        }
        if (!frame || typeof frame !== "object") {
          current.close();
          return;
        }
        if (ready) {
          settle(frame);
          return;
        }
        if (!frame.session || !/^[a-f0-9]{64}$/.test(frame.session)) {
          reject(new Error("Sage browser bridge pairing refused"));
          current.close();
          return;
        }
        // A tab-scoped credential survives reload; the backend keeps it only for this run.
        try {
          sessionStorage.setItem(
            STORAGE_KEY,
            JSON.stringify({ port, session: frame.session }),
          );
        } catch {
          reject(new Error("Sage browser bridge requires tab session storage"));
          current.close();
          return;
        }
        clearTimeout(timer);
        ready = true;
        resolve();
      });
    });

  // A reloaded tab can reach the backend before the previous page's session
  // has closed, so session reconnects retry briefly. A pairing code is single-use.
  for (let attempt = 1; ; attempt++) {
    try {
      await pair();
      break;
    } catch (error) {
      if (pairingCode || attempt === RECONNECT_ATTEMPTS) throw error;
      await new Promise((wait) => setTimeout(wait, RECONNECT_DELAY_MS));
    }
  }

  const unregisterListener = (_event: string, eventId: number) => {
    callbacks.delete(eventId);
  };
  const invoke = async (
    cmd: string,
    args: Record<string, unknown> = {},
  ): Promise<unknown> => {
    try {
      const current = socket;
      if (!ready || !current || current.readyState !== WebSocket.OPEN)
        throw new Error("Sage browser bridge disconnected");
      if (pending.size >= MAX_PENDING)
        throw new Error("Sage browser bridge invoke limit reached");
      const id = ++sequence;
      const body = JSON.stringify({ id, cmd, args });
      if (new Blob([body]).size + current.bufferedAmount > MAX_MESSAGE_BYTES)
        throw new Error("Sage browser bridge message limit reached");
      return await new Promise<unknown>((resolve, reject) => {
        pending.set(id, { resolve, reject });
        try {
          current.send(body);
        } catch (error) {
          pending.delete(id);
          reject(error);
        }
      });
    } catch (error) {
      if (cmd === "plugin:event|listen" && typeof args.handler === "number")
        callbacks.delete(args.handler);
      throw error;
    }
  };
  Object.assign(window, {
    __TAURI_INTERNALS__: {
      invoke,
      transformCallback(
        run: (value: unknown) => void = () => {},
        once = false,
      ) {
        if (callbacks.size >= MAX_CALLBACKS)
          throw new Error("Sage browser bridge callback limit reached");
        const id = ++sequence;
        callbacks.set(id, { run, once });
        return id;
      },
      unregisterCallback(id: number) {
        callbacks.delete(id);
      },
      metadata: {
        currentWindow: { label: "main" },
        currentWebview: { label: "main" },
      },
      convertFileSrc() {
        throw new Error(
          "Native asset URLs are unavailable in the browser bridge",
        );
      },
    },
    __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener },
  });
}

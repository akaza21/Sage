const MAX_PENDING = 64;
const MAX_CALLBACKS = 256;
const MAX_MESSAGE_BYTES = 4 * 1024 * 1024;
const AUTH_TIMEOUT_MS = 10_000;
const BRIDGE_NAME = "SAGE_DEV_BROWSER_BRIDGE";

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

export async function installBrowserBridge(): Promise<void> {
  const pairingCode = location.hash.slice(1);
  history.replaceState(history.state, "", location.pathname + location.search);
  const port = new URLSearchParams(location.search).get("bridge") ?? "";
  if (!/^\d{1,5}$/.test(port) || Number(port) < 1 || Number(port) > 65535)
    throw new Error(`${BRIDGE_NAME}: invalid port`);
  const storageKey = "sage-browser-bridge:session";
  const saved = sessionStorage.getItem(storageKey);
  let session: string | undefined;
  if (saved) {
    try {
      const credential = JSON.parse(saved) as {
        port?: string;
        session?: string;
      };
      if (credential.port === port) session = credential.session;
    } catch {
      sessionStorage.removeItem(storageKey);
    }
  }
  if (!pairingCode && !session)
    throw new Error("Open the pairing link from Sage");
  const socket = new WebSocket(`ws://127.0.0.1:${port}`);
  const pending = new Map<number, Pending>();
  const callbacks = new Map<number, Callback>();
  let sequence = 0;
  let ready = false;

  await new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => {
      reject(new Error("Sage browser bridge pairing timed out"));
      socket.close();
    }, AUTH_TIMEOUT_MS);
    const disconnected = () => {
      clearTimeout(timer);
      ready = false;
      const error = new Error("Sage browser bridge disconnected");
      reject(error);
      for (const request of pending.values()) request.reject(error);
      pending.clear();
      callbacks.clear();
    };
    socket.addEventListener("close", disconnected);
    socket.addEventListener("error", () => {
      disconnected();
      socket.close();
    });
    socket.addEventListener(
      "open",
      () => {
        // Pairing credentials are sent only in the first frame, never a URL or invoke.
        socket.send(
          JSON.stringify(pairingCode ? { pairingCode } : { session }),
        );
      },
      { once: true },
    );
    socket.addEventListener("message", (event: MessageEvent) => {
      if (
        typeof event.data !== "string" ||
        new Blob([event.data]).size > MAX_MESSAGE_BYTES
      ) {
        socket.close();
        return;
      }
      let frame: Frame;
      try {
        frame = JSON.parse(event.data) as Frame;
      } catch {
        socket.close();
        return;
      }
      if (!frame || typeof frame !== "object") {
        socket.close();
        return;
      }
      if (!ready) {
        if (!frame.session || !/^[a-f0-9]{64}$/.test(frame.session)) {
          reject(new Error("Sage browser bridge pairing refused"));
          socket.close();
          return;
        }
        // A tab-scoped credential survives reload; the backend retains it only for this run.
        try {
          sessionStorage.setItem(
            storageKey,
            JSON.stringify({ port, session: frame.session }),
          );
        } catch {
          reject(new Error("Sage browser bridge requires tab session storage"));
          socket.close();
          return;
        }
        clearTimeout(timer);
        ready = true;
        resolve();
        return;
      }
      if (
        typeof frame.event === "string" &&
        typeof frame.handlerId === "number"
      ) {
        const callback = callbacks.get(frame.handlerId);
        if (callback) {
          if (callback.once) callbacks.delete(frame.handlerId);
          callback.run({
            event: frame.event,
            id: frame.handlerId,
            payload: frame.payload,
          });
        }
        return;
      }
      if (typeof frame.id !== "number") return;
      const request = pending.get(frame.id);
      if (!request) return;
      pending.delete(frame.id);
      if (Object.hasOwn(frame, "err")) request.reject(frame.err);
      else request.resolve(frame.ok);
    });
  });

  const unregisterListener = (_event: string, eventId: number) => {
    callbacks.delete(eventId);
  };
  const invoke = async (
    cmd: string,
    args: Record<string, unknown> = {},
  ): Promise<unknown> => {
    try {
      if (!ready || socket.readyState !== 1)
        throw new Error("Sage browser bridge disconnected");
      if (pending.size >= MAX_PENDING)
        throw new Error("Sage browser bridge invoke limit reached");
      const id = ++sequence;
      const body = JSON.stringify({ id, cmd, args });
      if (
        new Blob([body]).size > MAX_MESSAGE_BYTES ||
        socket.bufferedAmount > MAX_MESSAGE_BYTES
      )
        throw new Error("Sage browser bridge message limit reached");
      const result = await new Promise<unknown>((resolve, reject) => {
        pending.set(id, { resolve, reject });
        try {
          socket.send(body);
        } catch (error) {
          pending.delete(id);
          reject(error);
        }
      });
      return result;
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

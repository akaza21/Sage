import { pollBridgeRuns, showBridgeNotice } from "./bridgeRun";
import { MAX_MESSAGE_BYTES } from "./limits";

const MAX_PENDING = 64;
const MAX_CALLBACKS = 256;
const AUTH_TIMEOUT_MS = 10_000;
const RECONNECT_ATTEMPTS = 5;
const RECONNECT_DELAY_MS = 400;
// Connection attempts per published run before the tab waits for another run.
const RUN_ATTEMPTS = 5;
// How long a code-less run may stay current before the tab gives up on it.
const ANOTHER_TAB_WAIT_MS = 5_000;
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

type Target = {
  port: string;
  /** Set when the tab paired from a published run (`npm run dev`). */
  run?: string;
  credential: { pairingCode: string } | { session: string };
};

function savedSession(port: string, run?: string): string | undefined {
  const saved = sessionStorage.getItem(STORAGE_KEY);
  if (!saved) return undefined;
  try {
    const credential = JSON.parse(saved) as {
      port?: string;
      run?: string;
      session?: string;
    };
    return credential.port === port && credential.run === run
      ? credential.session
      : undefined;
  } catch {
    sessionStorage.removeItem(STORAGE_KEY);
    return undefined;
  }
}

const showDisconnected = () =>
  showBridgeNotice(
    "Sage browser bridge disconnected. Reload this tab to reconnect.",
    true,
  );

/** A pairing link names its port; without one the tab uses the published run. */
function linkTarget(pairingCode: string): Target | undefined {
  const linkPort = new URLSearchParams(location.search).get("bridge");
  if (linkPort === null) return undefined;
  if (
    !/^\d{1,5}$/.test(linkPort) ||
    Number(linkPort) < 1 ||
    Number(linkPort) > 65535
  )
    throw new Error(`${BRIDGE_NAME}: invalid port`);
  const session = savedSession(linkPort);
  if (pairingCode) return { port: linkPort, credential: { pairingCode } };
  if (session) return { port: linkPort, credential: { session } };
  throw new Error("Open the pairing link from Sage");
}

/**
 * Pairs with the published run. The record can outlive its backend (stopped,
 * or rebuilding before the next run listens), so a failed attempt returns to
 * discovery; each run gets a bounded number of attempts.
 */
async function pairPublishedRun(
  pair: (target: Target) => Promise<void>,
): Promise<Target> {
  const clear = showBridgeNotice("Waiting for the Sage backend to start…");
  let lastRun: string | undefined;
  let tried = 0;
  let seenSince = Date.now();
  try {
    return await pollBridgeRuns(async ({ run, bridgePort, code }) => {
      if (run !== lastRun) [lastRun, tried, seenSince] = [run, 0, Date.now()];
      const port = String(bridgePort);
      const session = savedSession(port, run);
      // A reload of a paired tab resumes its session; a fresh run pairs by code.
      const credential = session
        ? { session }
        : code
          ? { pairingCode: code }
          : undefined;
      if (!credential) {
        // The code is single-use, so this tab can never pair with this run. A
        // rebuild may be about to publish the next one; past a short wait the
        // run really belongs to another tab.
        if (Date.now() - seenSince < ANOTHER_TAB_WAIT_MS) return undefined;
        throw new Error(
          "This backend is paired with another tab; close it or restart npm run dev.",
        );
      }
      if (tried >= RUN_ATTEMPTS) return undefined;
      tried++;
      const target = { port, run, credential };
      try {
        await pair(target);
        return target;
      } catch {
        return undefined;
      }
    });
  } finally {
    clear();
  }
}

export async function installBrowserBridge(
  pairingCode: string,
  { reload = () => location.reload() }: { reload?: () => void } = {},
): Promise<void> {
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

  const pair = ({ port, run, credential }: Target) =>
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
        if (run) recover(run);
        else showDisconnected();
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
          current.send(JSON.stringify(credential));
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
            JSON.stringify({ port, run, session: frame.session }),
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

  /**
   * A dropped socket: retry this run's session while its backend may still be
   * alive, else wait for the next run. Either way the page reloads, so the app
   * subscribes afresh to whichever backend answered.
   */
  const recover = (current: string) => {
    const clear = showBridgeNotice("Reconnecting to the Sage backend…");
    let tried = 0;
    void pollBridgeRuns(async ({ run, bridgePort, code }) => {
      if (run !== current) return code === undefined ? undefined : true;
      const session = savedSession(String(bridgePort), run);
      if (!session || tried >= RUN_ATTEMPTS) return undefined;
      tried++;
      try {
        await pair({ port: String(bridgePort), run, credential: { session } });
      } catch {
        return undefined;
      }
      // Free the session for the reloaded page.
      ready = false;
      socket?.close();
      return true;
    }).then(reload, () => {
      clear();
      showDisconnected();
    });
  };

  const link = linkTarget(pairingCode);
  if (!link) await pairPublishedRun(pair);
  // A reloaded tab can reach the backend before the previous page's session
  // has closed, so session reconnects retry briefly. A pairing code is single-use.
  else
    for (let attempt = 1; ; attempt++) {
      try {
        await pair(link);
        break;
      } catch (error) {
        if ("pairingCode" in link.credential || attempt === RECONNECT_ATTEMPTS)
          throw error;
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

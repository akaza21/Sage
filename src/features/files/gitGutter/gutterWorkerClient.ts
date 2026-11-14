import type { GutterRequest, GutterResult } from "./gutterWorkerCore";

export interface GutterConnection {
  readonly id: number;
  post: (request: GutterRequest) => void;
  close: () => void;
}

export type GutterWorkerResult = GutterResult | { id: number; failed: true };

type Listener = (result: GutterWorkerResult) => void;

export const GUTTER_WORKER_LINGER_MS = 5_000;

export function createGutterWorkerPool(
  spawn: () => Worker,
  lingerMs = GUTTER_WORKER_LINGER_MS,
) {
  let worker: Worker | null = null;
  let stopTimer: ReturnType<typeof setTimeout> | null = null;
  let nextId = 1;
  const listeners = new Map<number, Listener>();

  const stop = () => {
    if (stopTimer !== null) clearTimeout(stopTimer);
    stopTimer = null;
    if (worker) {
      worker.onmessage = null;
      worker.onerror = null;
      worker.onmessageerror = null;
      worker.terminate();
    }
    worker = null;
  };

  const fail = () => {
    stop();
    // Defer notification: postMessage can throw during an editor update.
    for (const [id, listener] of listeners) {
      queueMicrotask(() => {
        if (listeners.get(id) === listener) listener({ id, failed: true });
      });
    }
  };

  function ensureWorker(): Worker | null {
    if (worker) return worker;
    try {
      worker = spawn();
    } catch {
      fail();
      return null;
    }
    worker.onmessage = (event: MessageEvent<GutterResult>) =>
      listeners.get(event.data.id)?.(event.data);
    worker.onerror = fail;
    worker.onmessageerror = fail;
    return worker;
  }

  return {
    connect(listener: Listener): GutterConnection {
      const id = nextId++;
      listeners.set(id, listener);
      if (stopTimer !== null) clearTimeout(stopTimer);
      stopTimer = null;
      return {
        id,
        post: (request) => {
          if (!listeners.has(id)) return;
          try {
            ensureWorker()?.postMessage(request);
          } catch {
            fail();
          }
        },
        close: () => {
          if (!listeners.delete(id)) return;
          try {
            worker?.postMessage({ type: "close", id });
          } catch {
            fail();
          }
          if (listeners.size === 0 && worker && stopTimer === null)
            stopTimer = setTimeout(stop, lingerMs);
        },
      };
    },
    running: () => worker !== null,
  };
}

let shared: ReturnType<typeof createGutterWorkerPool> | null = null;

export function sharedGutterWorkerPool() {
  shared ??= createGutterWorkerPool(
    () =>
      new Worker(new URL("./gutterWorker.ts", import.meta.url), {
        type: "module",
      }),
  );
  return shared;
}

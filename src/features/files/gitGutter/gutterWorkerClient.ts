import type { GutterRequest, GutterResult } from "./gutterWorkerCore";

export interface GutterConnection {
  readonly id: number;
  post: (request: GutterRequest) => void;
  /** Closes this editor's document; the last close stops the worker. */
  close: () => void;
}

type Listener = (result: GutterResult) => void;

/**
 * One worker shared by every editor with a gutter (EP-14 budget). It starts
 * with the first connection and is terminated with the last, so no worker
 * runs while no File is open.
 */
export function createGutterWorkerPool(spawn: () => Worker) {
  let worker: Worker | null = null;
  // A worker that failed to load is not respawned for every edit.
  let failed = false;
  let nextId = 1;
  const listeners = new Map<number, Listener>();

  const stop = () => {
    worker?.terminate();
    worker = null;
  };

  function ensureWorker(): Worker | null {
    if (worker || failed) return worker;
    try {
      worker = spawn();
    } catch {
      failed = true;
      return null;
    }
    worker.onmessage = (event: MessageEvent<GutterResult>) =>
      listeners.get(event.data.id)?.(event.data);
    // A worker that fails leaves the editors without markers, never broken.
    worker.onerror = () => {
      failed = true;
      stop();
    };
    return worker;
  }

  return {
    connect(listener: Listener): GutterConnection {
      const id = nextId++;
      listeners.set(id, listener);
      return {
        id,
        post: (request) => {
          if (listeners.has(id)) ensureWorker()?.postMessage(request);
        },
        close: () => {
          if (!listeners.delete(id)) return;
          if (listeners.size === 0) stop();
          else worker?.postMessage({ type: "close", id });
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

import { describe, expect, it, vi } from "vitest";

import {
  GUTTER_WORKER_LINGER_MS,
  createGutterWorkerPool,
} from "./gutterWorkerClient";

function fakeWorker() {
  return {
    onmessage: null as ((event: MessageEvent) => void) | null,
    onerror: null as (() => void) | null,
    postMessage: vi.fn(),
    terminate: vi.fn(),
  };
}

describe("gutter worker pool", () => {
  it("shares one worker and stops it a while after the last editor closes", () => {
    vi.useFakeTimers();
    const workers: ReturnType<typeof fakeWorker>[] = [];
    const pool = createGutterWorkerPool(() => {
      const worker = fakeWorker();
      workers.push(worker);
      return worker as unknown as Worker;
    });
    const first = vi.fn();
    const one = pool.connect(first);
    const two = pool.connect(vi.fn());
    expect(pool.running()).toBe(false);
    one.post({ type: "close", id: one.id });
    two.post({ type: "close", id: two.id });
    expect(workers).toHaveLength(1);

    workers[0]!.onmessage?.({
      data: { id: one.id, version: 1, hunks: [] },
    } as MessageEvent);
    expect(first).toHaveBeenCalledTimes(1);

    one.close();
    expect(workers[0]!.terminate).not.toHaveBeenCalled();
    expect(workers[0]!.postMessage).toHaveBeenLastCalledWith({
      type: "close",
      id: one.id,
    });
    two.close();
    expect(workers[0]!.terminate).not.toHaveBeenCalled();
    // A File opened meanwhile keeps the same worker.
    const three = pool.connect(vi.fn());
    vi.advanceTimersByTime(GUTTER_WORKER_LINGER_MS);
    three.post({ type: "close", id: three.id });
    expect(workers).toHaveLength(1);
    three.close();
    vi.advanceTimersByTime(GUTTER_WORKER_LINGER_MS);
    expect(workers[0]!.terminate).toHaveBeenCalled();
    expect(pool.running()).toBe(false);
    vi.useRealTimers();
  });

  it("does not respawn a worker that failed", () => {
    const spawn = vi.fn(() => fakeWorker() as unknown as Worker);
    const pool = createGutterWorkerPool(spawn);
    const connection = pool.connect(vi.fn());
    connection.post({ type: "close", id: connection.id });
    (spawn.mock.results[0]!.value as ReturnType<typeof fakeWorker>).onerror?.();
    connection.post({ type: "close", id: connection.id });
    expect(spawn).toHaveBeenCalledTimes(1);
  });
});

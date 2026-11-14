import { EditorView } from "@codemirror/view";
import { attachGitGutter, GUTTER_IDLE_MS } from "./gitGutterExtension";
import { gutterField, setGutterPeek } from "./gitGutterState";
import { createGutterWorkerCore, type GutterRequest } from "./gutterWorkerCore";
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

  it("clears every editor on failure and settles pending work so edits recover", async () => {
    vi.useFakeTimers();
    const workers: ReturnType<typeof fakeWorker>[] = [];
    let hold = false;
    const pool = createGutterWorkerPool(() => {
      const core = createGutterWorkerCore();
      const worker = fakeWorker();
      worker.postMessage.mockImplementation((request: GutterRequest) => {
        const result = core.handle(request);
        if (result && !hold)
          queueMicrotask(() =>
            worker.onmessage?.({ data: result } as MessageEvent),
          );
      });
      workers.push(worker);
      return worker as unknown as Worker;
    });
    const views = [
      new EditorView({ doc: "changed" }),
      new EditorView({ doc: "changed" }),
    ];
    const handles = views.map((view) => attachGitGutter(view, pool));
    try {
      handles.forEach((handle) => handle.setBase("base"));
      await vi.advanceTimersByTimeAsync(GUTTER_IDLE_MS);
      for (const view of views) {
        expect(view.state.field(gutterField).hunks).toHaveLength(1);
        expect(view.dom.querySelector(".git-gutter-marker")).not.toBeNull();
      }
      hold = true;
      for (const view of views)
        view.dispatch({ changes: { from: 1, insert: "x" } });
      await vi.advanceTimersByTimeAsync(GUTTER_IDLE_MS);
      for (const view of views) {
        view.dispatch({ effects: setGutterPeek.of(0) });
        expect(view.state.field(gutterField).peek).toBe(0);
      }
      workers[0]!.onerror?.();
      await vi.advanceTimersByTimeAsync(0);
      expect(pool.running()).toBe(false);
      for (const view of views) {
        expect(view.state.field(gutterField).hunks).toEqual([]);
        expect(view.state.field(gutterField).peek).toBeNull();
        expect(view.dom.querySelector(".git-gutter-marker")).toBeNull();
        expect(view.dom.querySelector(".git-gutter-peek")).toBeNull();
      }
      hold = false;
      for (const view of views)
        view.dispatch({ changes: { from: 1, insert: "y" } });
      await vi.advanceTimersByTimeAsync(GUTTER_IDLE_MS);
      for (const view of views)
        expect(view.state.field(gutterField).hunks).toHaveLength(1);
    } finally {
      handles.forEach((handle) => handle.destroy());
      views.forEach((view) => view.destroy());
      vi.useRealTimers();
    }
  });

  it.each(["spawn", "post"])(
    "reports %s exceptions and lets the next request recover",
    async (failure) => {
      let broken = true;
      const pool = createGutterWorkerPool(() => {
        if (broken && failure === "spawn") throw new Error("unavailable");
        const worker = fakeWorker();
        worker.postMessage.mockImplementation(() => {
          if (broken) throw new Error("unavailable");
        });
        return worker as unknown as Worker;
      });
      const results: unknown[] = [];
      const connection = pool.connect((result) => results.push(result));
      connection.post({
        type: "open",
        id: connection.id,
        version: 1,
        base: "",
        doc: "x",
      });
      await Promise.resolve();
      expect(results).toEqual([{ id: connection.id, failed: true }]);
      expect(pool.running()).toBe(false);
      broken = false;
      connection.post({
        type: "open",
        id: connection.id,
        version: 2,
        base: "",
        doc: "x",
      });
      expect(pool.running()).toBe(true);
      connection.close();
    },
  );
});

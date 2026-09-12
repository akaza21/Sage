import { describe, expect, it, vi } from "vitest";

const listenMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

import {
  acknowledgeReadTextFileFlush,
  subscribeAcpReadTextFileFlush,
  type AcpReadTextFileFlushEvent,
} from "./acpReadTextFileFlush";

const event: AcpReadTextFileFlushEvent = {
  owner: {
    projectId: "project",
    projectGeneration: "1",
    chatId: "chat",
    sessionId: "session",
    runtimeEpoch: "2",
  },
  resource: { rootId: "root", relativePath: "file.txt" },
  expectedEditorRevision: 5,
  token: "p02-1",
};

function acknowledgeFor() {
  return vi.fn().mockImplementation(async (ack) => {
    if (ack.state.kind !== "classify") return { kind: "complete" };
    if (ack.state.state === "persisted")
      return { kind: "persisted", editorRevision: ack.state.editorRevision };
    if (ack.state.state === "dirty")
      return {
        kind: "flushRequired",
        editorRevision: ack.state.editorRevision,
        previousDurableRevision: ack.expectedEditorRevision,
      };
    return {
      kind: "diskAllowed",
      state: ack.state.state,
      editorRevision: ack.state.editorRevision,
    };
  });
}

function ready(
  revision: number,
  persistedRevision: number,
  dirty = revision > persistedRevision,
) {
  return {
    phase: "ready" as const,
    resource: event.resource,
    content: "x",
    revision,
    persistedRevision,
    persisting: false,
    saving: false,
    message: null,
    document: {
      resource: event.resource,
      dirty,
      lineEnding: "lf" as const,
      disk: { fingerprint: "a", byteLen: 0, modifiedAt: null },
      base: { fingerprint: "a", byteLen: 0, modifiedAt: null },
      conflict: null,
      buffer: {
        content: "x",
        fingerprint: "a",
        byteLen: 1,
        revision,
      },
    },
  };
}

describe("P-02 editor flush acknowledgement", () => {
  it("subscribes and forwards classify then the terminal IPC-shaped acknowledgement", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
    const acknowledge = acknowledgeFor();
    let listener: ((received: { payload: typeof event }) => void) | undefined;
    listenMock.mockImplementation(async (_name, callback) => {
      listener = callback;
      return () => undefined;
    });
    const unsubscribe = await subscribeAcpReadTextFileFlush(
      () => ({
        projectId: "project",
        projectGeneration: 1,
        transitioning: false,
        activeEditor: null,
        currentEditor: () => null,
        flushActiveEditor: vi.fn(),
      }),
      acknowledge,
    );
    expect(typeof unsubscribe).toBe("function");
    expect(listenMock).toHaveBeenCalledWith(
      "acp-read-text-file-flush-v1",
      expect.any(Function),
    );
    listener?.({ payload: { ...event, expectedEditorRevision: null } });
    await vi.waitFor(() => expect(acknowledge).toHaveBeenCalledTimes(2));
    expect(acknowledge.mock.calls.map(([ack]) => ack.state)).toEqual([
      { kind: "classify", state: "notOpen", editorRevision: null },
      { kind: "diskAllowed", state: "notOpen", editorRevision: null },
    ]);
  });

  it("subscribes an active clean editor through classify then matching disk acknowledgement", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
    const acknowledge = acknowledgeFor();
    let listener: ((received: { payload: typeof event }) => void) | undefined;
    listenMock.mockImplementation(async (_name, callback) => {
      listener = callback;
      return () => undefined;
    });
    await subscribeAcpReadTextFileFlush(
      () => ({
        projectId: "project",
        projectGeneration: 1,
        transitioning: false,
        activeEditor: ready(7, 7, false),
        currentEditor: () => ready(7, 7, false),
        flushActiveEditor: vi.fn(),
      }),
      acknowledge,
    );
    listener?.({ payload: { ...event, expectedEditorRevision: null } });
    await vi.waitFor(() => expect(acknowledge).toHaveBeenCalledTimes(2));
    expect(acknowledge.mock.calls.map(([ack]) => ack.state)).toEqual([
      { kind: "classify", state: "openClean", editorRevision: 7 },
      { kind: "diskAllowed", state: "openClean", editorRevision: 7 },
    ]);
  });

  it("aborts a delayed clean disk action when the editor becomes dirty", async () => {
    let resolve!: (action: {
      kind: "diskAllowed";
      state: "openClean";
      editorRevision: number | null;
    }) => void;
    const acknowledge = vi.fn(
      () =>
        new Promise<
          typeof resolve extends (value: infer T) => void ? T : never
        >((done) => {
          resolve = done as typeof resolve;
        }),
    );
    let current = ready(7, 7, false);
    const pending = acknowledgeReadTextFileFlush(
      { ...event, expectedEditorRevision: null },
      {
        projectId: "project",
        projectGeneration: 1,
        transitioning: false,
        activeEditor: current,
        currentEditor: () => current,
        flushActiveEditor: vi.fn(),
      },
      acknowledge,
    );
    await vi.waitFor(() => expect(acknowledge).toHaveBeenCalledOnce());
    current = ready(8, 7, true);
    resolve({ kind: "diskAllowed", state: "openClean", editorRevision: 7 });
    await pending;
    expect(acknowledge).toHaveBeenCalledOnce();
  });

  it("aborts delayed not-open and persisted actions when the target becomes active dirty", async () => {
    for (const expectedEditorRevision of [null, 5] as const) {
      let resolve!: (
        action:
          | { kind: "diskAllowed"; state: "notOpen"; editorRevision: null }
          | { kind: "persisted"; editorRevision: number },
      ) => void;
      const acknowledge = vi.fn(
        () =>
          new Promise<
            typeof resolve extends (value: infer T) => void ? T : never
          >((done) => {
            resolve = done as typeof resolve;
          }),
      );
      let current: ReturnType<typeof ready> | null = null;
      const pending = acknowledgeReadTextFileFlush(
        { ...event, expectedEditorRevision },
        {
          projectId: "project",
          projectGeneration: 1,
          transitioning: false,
          activeEditor: null,
          currentEditor: () => current,
          flushActiveEditor: vi.fn(),
        },
        acknowledge,
      );
      await vi.waitFor(() => expect(acknowledge).toHaveBeenCalledOnce());
      current = ready(8, 7, true);
      resolve(
        expectedEditorRevision === null
          ? { kind: "diskAllowed", state: "notOpen", editorRevision: null }
          : { kind: "persisted", editorRevision: 5 },
      );
      await pending;
      expect(acknowledge).toHaveBeenCalledOnce();
    }
  });

  it("uses notOpen only for a nonmatching editor", async () => {
    const acknowledge = acknowledgeFor();
    await acknowledgeReadTextFileFlush(
      { ...event, expectedEditorRevision: null },
      {
        projectId: "project",
        projectGeneration: 1,
        transitioning: false,
        activeEditor: null,
        currentEditor: () => null,
        flushActiveEditor: vi.fn(),
      },
      acknowledge,
    );
    expect(acknowledge).toHaveBeenCalledWith(
      expect.objectContaining({
        expectedEditorRevision: null,
        state: { kind: "diskAllowed", state: "notOpen", editorRevision: null },
      }),
    );
  });

  it("settles an inactive persisted dirty/recovered target from its native revision", async () => {
    const acknowledge = acknowledgeFor();
    await acknowledgeReadTextFileFlush(
      event,
      {
        projectId: "project",
        projectGeneration: 1,
        transitioning: false,
        activeEditor: {
          ...ready(6, 6, false),
          resource: { rootId: "root", relativePath: "other.txt" },
        },
        currentEditor: () => null,
        flushActiveEditor: vi.fn(),
      },
      acknowledge,
    );
    expect(acknowledge).toHaveBeenCalledWith(
      expect.objectContaining({
        state: { kind: "persisted", editorRevision: 5, durableRevision: 5 },
      }),
    );
  });

  it("flushes dirty current content and rejects a revision raced after flush", async () => {
    const acknowledge = acknowledgeFor();
    const dirty = ready(5, 4);
    const bridge = {
      projectId: "project",
      projectGeneration: 1,
      transitioning: false,
      activeEditor: dirty,
      flushActiveEditor: vi.fn().mockResolvedValue(undefined),
      currentEditor: () => ready(6, 5),
    };
    await acknowledgeReadTextFileFlush(event, bridge, acknowledge);
    expect(bridge.flushActiveEditor).toHaveBeenCalledOnce();
    expect(acknowledge).toHaveBeenCalledOnce();
  });

  it("does not emit a terminal acknowledgement after a delayed flush changes revision", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
    const acknowledge = acknowledgeFor();
    let listener: ((received: { payload: typeof event }) => void) | undefined;
    listenMock.mockImplementation(async (_name, callback) => {
      listener = callback;
      return () => undefined;
    });
    const flush = vi.fn().mockResolvedValue(undefined);
    await subscribeAcpReadTextFileFlush(
      () => ({
        projectId: "project",
        projectGeneration: 1,
        transitioning: false,
        activeEditor: ready(5, 4),
        currentEditor: () => ready(6, 5),
        flushActiveEditor: flush,
      }),
      acknowledge,
    );
    listener?.({ payload: event });
    await vi.waitFor(() => expect(flush).toHaveBeenCalledOnce());
    expect(acknowledge.mock.calls.map(([ack]) => ack.state)).toEqual([
      { kind: "classify", state: "dirty", editorRevision: 5 },
    ]);
  });

  it("does not construct a duplicate or late terminal acknowledgement from mismatched native actions", async () => {
    const acknowledge = vi.fn().mockResolvedValue({
      kind: "diskAllowed",
      state: "openClean",
      editorRevision: 9,
    });
    await acknowledgeReadTextFileFlush(
      { ...event, expectedEditorRevision: null },
      {
        projectId: "project",
        projectGeneration: 1,
        transitioning: false,
        activeEditor: ready(7, 7, false),
        currentEditor: () => ready(7, 7, false),
        flushActiveEditor: vi.fn(),
      },
      acknowledge,
    );
    expect(acknowledge.mock.calls.map(([ack]) => ack.state)).toEqual([
      { kind: "classify", state: "openClean", editorRevision: 7 },
    ]);
  });

  it("acknowledges an exactly flushed dirty editor without content", async () => {
    const acknowledge = acknowledgeFor();
    const dirty = ready(5, 4);
    await acknowledgeReadTextFileFlush(
      event,
      {
        projectId: "project",
        projectGeneration: 1,
        transitioning: false,
        activeEditor: dirty,
        flushActiveEditor: vi.fn().mockResolvedValue(undefined),
        currentEditor: () => ready(5, 5),
      },
      acknowledge,
    );
    expect(acknowledge).toHaveBeenCalledWith(
      expect.objectContaining({
        expectedEditorRevision: 5,
        state: {
          kind: "persisted",
          editorRevision: 5,
          durableRevision: 5,
        },
      }),
    );
    expect(JSON.stringify(acknowledge.mock.calls[0][0])).not.toContain(
      "content",
    );
  });

  it("never falls back to disk for recovered dirty content at an equal revision", async () => {
    const acknowledge = acknowledgeFor();
    const recoveredDirty = ready(5, 5, true);
    await acknowledgeReadTextFileFlush(
      event,
      {
        projectId: "project",
        projectGeneration: 1,
        transitioning: false,
        activeEditor: recoveredDirty,
        flushActiveEditor: vi.fn().mockResolvedValue(undefined),
        currentEditor: () => ready(5, 5, true),
      },
      acknowledge,
    );
    expect(acknowledge).toHaveBeenCalledWith(
      expect.objectContaining({
        state: {
          kind: "persisted",
          editorRevision: 5,
          durableRevision: 5,
        },
      }),
    );
  });

  it("never falls back to disk after a dirty revision was already snapshotted", async () => {
    const acknowledge = acknowledgeFor();
    const snapshottedDirty = ready(5, 5, true);
    await acknowledgeReadTextFileFlush(
      event,
      {
        projectId: "project",
        projectGeneration: 1,
        transitioning: false,
        activeEditor: snapshottedDirty,
        flushActiveEditor: vi.fn().mockResolvedValue(undefined),
        currentEditor: () => ready(5, 5, true),
      },
      acknowledge,
    );
    expect(acknowledge.mock.calls[0][0].state.kind).toBe("classify");
    expect(acknowledge.mock.calls[1][0].state.kind).toBe("persisted");
  });
});

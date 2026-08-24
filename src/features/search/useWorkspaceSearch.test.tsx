import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { resetWorkspaceNotifyComposeForTests } from "../files/workspaceNotifyCompose";

import type { WorkspaceSearchResponse } from "./workspaceSearchDomain";
import { useWorkspaceSearch } from "./useWorkspaceSearch";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => (resolve = done));
  return { promise, resolve };
}

function response(query: string): WorkspaceSearchResponse {
  return {
    requestId: "request-a",
    projectId: "project-a",
    transitionEpoch: 4,
    indexGeneration: 0,
    query,
    matches: [],
    roots: [
      {
        rootId: "root-a",
        rootName: "Sage",
        state: "complete",
        filesScanned: 0,
        bytesScanned: 0,
        entriesVisited: 0,
        skippedFiles: 0,
        errorCount: 0,
        message: null,
      },
    ],
    truncated: false,
    continuationToken: null,
    limitReasons: [],
    elapsedMs: 1,
  };
}

describe("workspace Search state", () => {
  afterEach(() => {
    resetWorkspaceNotifyComposeForTests();
  });

  it("aborts an older scan and never projects its stale response", async () => {
    const first = deferred<WorkspaceSearchResponse>();
    const second = deferred<WorkspaceSearchResponse>();
    const search = vi
      .fn()
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    const { result } = renderHook(() =>
      useWorkspaceSearch({
        projectId: "project-a",
        transitionEpoch: 4,
        indexGeneration: 0,
        gateway: { search },
      }),
    );

    act(() => result.current.setQuery("first"));
    act(() => void result.current.submit());
    act(() => result.current.setQuery("second"));
    act(() => void result.current.submit());

    expect(search.mock.calls[0][1].aborted).toBe(true);
    await act(async () => second.resolve(response("second")));
    await act(async () => first.resolve(response("first")));

    expect(result.current.state.phase).toBe("results");
    expect(result.current.state.response?.query).toBe("second");
  });

  it("cancels and clears disposable results when the active Project changes", async () => {
    const pending = deferred<WorkspaceSearchResponse>();
    const search = vi.fn().mockReturnValue(pending.promise);
    const { result, rerender } = renderHook(
      ({ projectId, transitionEpoch }) =>
        useWorkspaceSearch({
          projectId,
          transitionEpoch,
          indexGeneration: 0,
          gateway: { search },
        }),
      {
        initialProps: {
          projectId: "project-a" as string | null,
          transitionEpoch: 4,
        },
      },
    );
    act(() => result.current.setQuery("needle"));
    act(() => void result.current.submit());

    rerender({ projectId: "project-b", transitionEpoch: 5 });

    expect(search.mock.calls[0][1].aborted).toBe(true);
    expect(result.current.state).toMatchObject({ phase: "idle", query: "" });
  });

  it("retires a pending response as soon as the query criteria changes", async () => {
    const pending = deferred<WorkspaceSearchResponse>();
    const search = vi.fn().mockReturnValue(pending.promise);
    const { result } = renderHook(() =>
      useWorkspaceSearch({
        projectId: "project-a",
        transitionEpoch: 4,
        indexGeneration: 0,
        gateway: { search },
      }),
    );
    act(() => result.current.setQuery("first"));
    act(() => void result.current.submit());
    const signal = search.mock.calls[0][1] as AbortSignal;

    act(() => result.current.setQuery("second"));
    expect(signal.aborted).toBe(true);
    expect(result.current.state).toMatchObject({
      phase: "idle",
      query: "second",
      response: null,
    });

    await act(async () => pending.resolve(response("first")));
    expect(result.current.state.response).toBeNull();
  });

  it("retires visible results when case sensitivity changes", async () => {
    const search = vi.fn().mockResolvedValue(response("needle"));
    const { result } = renderHook(() =>
      useWorkspaceSearch({
        projectId: "project-a",
        transitionEpoch: 4,
        indexGeneration: 0,
        gateway: { search },
      }),
    );
    act(() => result.current.setQuery("needle"));
    await act(async () => void (await result.current.submit()));
    expect(result.current.state.phase).toBe("results");

    act(() => result.current.setCaseSensitive(true));
    expect(result.current.state).toMatchObject({
      phase: "idle",
      caseSensitive: true,
      response: null,
    });
  });

  it.each([
    ["Project ID", { projectId: "project-b" }],
    ["transition epoch", { transitionEpoch: 5 }],
    ["query", { query: "other" }],
  ])(
    "rejects a response with the wrong %s without retaining it",
    async (_identity, mismatch) => {
      const search = vi
        .fn()
        .mockResolvedValue({ ...response("needle"), ...mismatch });
      const { result } = renderHook(() =>
        useWorkspaceSearch({
          projectId: "project-a",
          transitionEpoch: 4,
          indexGeneration: 0,
          gateway: { search },
        }),
      );

      act(() => result.current.setQuery("needle"));
      await act(async () => void (await result.current.submit()));

      expect(result.current.state).toMatchObject({
        phase: "error",
        response: null,
        error: "invalid workspace Search response",
      });
    },
  );

  it("rejects an over-limit gateway response without retaining it", async () => {
    const match = {
      resource: { rootId: "root-a", relativePath: "safe.txt" },
      lineNumber: 1,
      columnNumber: 1,
      preview: "needle",
    };
    const search = vi.fn().mockResolvedValue({
      ...response("needle"),
      matches: Array.from({ length: 501 }, () => match),
    });
    const { result } = renderHook(() =>
      useWorkspaceSearch({
        projectId: "project-a",
        transitionEpoch: 4,
        indexGeneration: 0,
        gateway: { search },
      }),
    );

    act(() => result.current.setQuery("needle"));
    await act(async () => void (await result.current.submit()));

    expect(result.current.state).toMatchObject({
      phase: "error",
      response: null,
      error: "invalid workspace Search response",
    });
  });

  it("resubmits a live query when an accepted workspace notice arrives", async () => {
    const liveResponse = {
      ...response("needle"),
      transitionEpoch: 7,
      indexGeneration: 3,
    };
    const search = vi
      .fn()
      .mockResolvedValueOnce(liveResponse)
      .mockResolvedValueOnce({
        ...liveResponse,
        requestId: "request-b",
      });
    const { result } = renderHook(() =>
      useWorkspaceSearch({
        projectId: "project-a",
        transitionEpoch: 7,
        indexGeneration: 3,
        gateway: { search },
      }),
    );
    act(() => result.current.setQuery("needle"));
    await act(async () => void (await result.current.submit()));
    expect(search).toHaveBeenCalledTimes(1);

    const { publishWorkspaceNotifySearch } = await import(
      "../files/workspaceNotifyCompose"
    );
    await act(async () => {
      publishWorkspaceNotifySearch({
        projectId: "project-a",
        projectGeneration: 3,
        transitionEpoch: 7,
        rootId: "root-a",
        relativePath: "src/main.ts",
        kind: "changed",
      });
      await Promise.resolve();
    });
    expect(search).toHaveBeenCalledTimes(2);
  });

  it("does not search an unsubmitted draft when a notice arrives", async () => {
    const search = vi.fn().mockResolvedValue(response("needle"));
    const { result } = renderHook(() =>
      useWorkspaceSearch({
        projectId: "project-a",
        transitionEpoch: 7,
        indexGeneration: 3,
        gateway: { search },
      }),
    );
    act(() => result.current.setQuery("needle"));
    const { publishWorkspaceNotifySearch } = await import(
      "../files/workspaceNotifyCompose"
    );
    await act(async () => {
      publishWorkspaceNotifySearch({
        projectId: "project-a",
        projectGeneration: 3,
        transitionEpoch: 7,
        rootId: "root-a",
        relativePath: "src/main.ts",
        kind: "changed",
      });
      await Promise.resolve();
    });
    expect(search).not.toHaveBeenCalled();
  });
});

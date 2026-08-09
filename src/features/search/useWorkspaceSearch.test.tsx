import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { WorkspaceSearchResponse } from "./workspaceSearchDomain";
import { useWorkspaceSearch } from "./useWorkspaceSearch";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => (resolve = done));
  return { promise, resolve };
}

function response(query: string): WorkspaceSearchResponse {
  return {
    projectId: "project-a",
    query,
    matches: [],
    roots: [],
    truncated: false,
    limitReasons: [],
    elapsedMs: 1,
  };
}

describe("workspace Search state", () => {
  it("aborts an older scan and never projects its stale response", async () => {
    const first = deferred<WorkspaceSearchResponse>();
    const second = deferred<WorkspaceSearchResponse>();
    const search = vi
      .fn()
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    const { result } = renderHook(() =>
      useWorkspaceSearch({ projectId: "project-a", gateway: { search } }),
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
      ({ projectId }) => useWorkspaceSearch({ projectId, gateway: { search } }),
      { initialProps: { projectId: "project-a" as string | null } },
    );
    act(() => result.current.setQuery("needle"));
    act(() => void result.current.submit());

    rerender({ projectId: "project-b" });

    expect(search.mock.calls[0][1].aborted).toBe(true);
    expect(result.current.state).toMatchObject({ phase: "idle", query: "" });
  });
});

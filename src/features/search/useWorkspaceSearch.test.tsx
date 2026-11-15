import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import { useEditorSettingsStore } from "../../app/editorSettingsStore";
import { pathGlobMatcher } from "../../domain/pathGlobMatcher";
import {
  resetWorkspaceNotifyComposeForTests,
  syncLiveProjectWorkspaceNotify,
  type WorkspaceNotifyHooks,
  type WorkspaceNotifyNotice,
} from "../files/workspaceNotifyCompose";

import type {
  WorkspaceSearchGateway,
  WorkspaceSearchResponse,
} from "./workspaceSearchDomain";
import { useWorkspaceSearch } from "./useWorkspaceSearch";
import { loadWorkspaceSearchChecks } from "./workspaceSearchDesktop";

// The panel preloads the Search checks on mount; load them once up front so
// each Search settles within one act().
beforeAll(() => loadWorkspaceSearchChecks());

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

    const { publishWorkspaceNotifySearch } =
      await import("../files/workspaceNotifyCompose");
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

  it("updates ignored Search results after an edit and deletion when ignore rules are disabled", async () => {
    vi.useFakeTimers();
    const settings = useEditorSettingsStore.getState().settings;
    useEditorSettingsStore.setState({
      settings: { ...settings, "search.useIgnoreFiles": false },
    });
    let content: string | null = "needle before";
    const gateway: WorkspaceSearchGateway = {
      async search(request) {
        return {
          ...response(request.query),
          transitionEpoch: 7,
          indexGeneration: 3,
          matches:
            request.useIgnoreFiles === false && content?.includes(request.query)
              ? [
                  {
                    resource: { rootId: "root-a", relativePath: "dist/app.js" },
                    lineNumber: 1,
                    columnNumber: 1,
                    preview: content,
                  },
                ]
              : [],
        };
      },
    };
    let emit: (value: WorkspaceNotifyNotice) => void = () => undefined;
    const hooks: WorkspaceNotifyHooks = {
      watch: async () => ({ generation: 1 }),
      unwatch: async () => undefined,
      async listen(listener) {
        emit = listener;
        return () => undefined;
      },
      invalidate: () => new Promise<void>(() => undefined),
    };
    const { result, unmount } = renderHook(() =>
      useWorkspaceSearch({
        projectId: "project-a",
        transitionEpoch: 7,
        indexGeneration: 3,
        gateway,
      }),
    );
    const ignored = {
      projectId: "project-a",
      projectGeneration: 3,
      transitionEpoch: 7,
      rootId: "root-a",
      relativePath: "dist",
      kind: "ignored",
    };
    try {
      await syncLiveProjectWorkspaceNotify(
        {
          projectId: "project-a",
          projectGeneration: 3,
          transitionEpoch: 7,
          roots: [{ rootId: "root-a" }],
        },
        hooks,
      );
      act(() => result.current.setQuery("needle"));
      act(() => result.current.setIncludePatternText("dist/"));
      await act(async () => result.current.submit());
      expect(result.current.state.response?.matches[0]?.preview).toBe(
        "needle before",
      );

      content = "needle after";
      await act(async () => emit(ignored));
      expect(result.current.state.response?.matches[0]?.preview).toBe(
        "needle after",
      );

      content = null;
      act(() => {
        for (let index = 0; index < 50; index += 1) emit(ignored);
      });
      await act(async () => vi.advanceTimersByTimeAsync(999));
      expect(result.current.state.response?.matches[0]?.preview).toBe(
        "needle after",
      );
      await act(async () => vi.advanceTimersByTimeAsync(1));
      expect(result.current.state.phase).toBe("results");
      expect(result.current.state.response?.matches).toEqual([]);
    } finally {
      unmount();
      await syncLiveProjectWorkspaceNotify(null, hooks);
      useEditorSettingsStore.setState({ settings });
      vi.useRealTimers();
    }
  });

  it("removes existing matches when exclusion rules trigger a root resync", async () => {
    const settings = useEditorSettingsStore.getState().settings;
    useEditorSettingsStore.setState({
      settings: { ...settings, "search.useIgnoreFiles": true },
    });
    let excluded = false;
    const match = {
      resource: { rootId: "root-a", relativePath: "scratch/notes.md" },
      lineNumber: 1,
      columnNumber: 1,
      preview: "needle",
    };
    const gateway: WorkspaceSearchGateway = {
      async search(request) {
        return {
          ...response(request.query),
          indexGeneration: 1,
          matches: excluded && request.useIgnoreFiles !== false ? [] : [match],
        };
      },
    };
    let emit: (value: WorkspaceNotifyNotice) => void = () => undefined;
    const hooks: WorkspaceNotifyHooks = {
      watch: async () => ({ generation: 1 }),
      unwatch: async () => undefined,
      async listen(listener) {
        emit = listener;
        return () => undefined;
      },
      invalidate() {},
    };
    const { result, unmount } = renderHook(() =>
      useWorkspaceSearch({
        projectId: "project-a",
        transitionEpoch: 4,
        indexGeneration: 1,
        gateway,
      }),
    );
    try {
      await syncLiveProjectWorkspaceNotify(
        {
          projectId: "project-a",
          projectGeneration: 1,
          transitionEpoch: 4,
          roots: [{ rootId: "root-a" }],
        },
        hooks,
      );
      act(() => result.current.setQuery("needle"));
      await act(async () => result.current.submit());
      expect(result.current.state.response?.matches).toEqual([match]);

      excluded = true;
      // The native relevance filter turns an in-place info/exclude edit into
      // this resync, since .git paths themselves never reach the WebView.
      await act(async () =>
        emit({
          projectId: "project-a",
          projectGeneration: 1,
          transitionEpoch: 4,
          rootId: "root-a",
          relativePath: "lost-notices",
          kind: "resync",
        }),
      );
      expect(result.current.state.phase).toBe("results");
      expect(result.current.state.response?.matches).toEqual([]);
    } finally {
      unmount();
      await syncLiveProjectWorkspaceNotify(null, hooks);
      useEditorSettingsStore.setState({ settings });
    }
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
    const { publishWorkspaceNotifySearch } =
      await import("../files/workspaceNotifyCompose");
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
  describe("include and exclude patterns", () => {
    async function sentFilters(include: string, exclude = "") {
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
      act(() => result.current.setIncludePatternText(include));
      act(() => result.current.setExcludePatternText(exclude));
      await act(async () => void (await result.current.submit()));
      return { search, state: result.current.state };
    }

    async function sentPatterns(include: string) {
      const { search } = await sentFilters(include);
      expect(search).toHaveBeenCalledTimes(1);
      return search.mock.calls[0][0].includePatterns as string[];
    }

    it.each([
      ["*.ts", ["**/*.ts"]],
      ["src", ["**/src", "**/src/**"]],
      ["dist/", ["dist/**"]],
      ["./src", ["src", "src/**"]],
      ["./*.md", ["*.md"]],
      ["src/**/*.ts", ["src/**/*.ts"]],
      ["*.{ts,tsx}", ["**/*.ts", "**/*.tsx"]],
      ["{src,lib}/*.ts, *.md", ["src/*.ts", "lib/*.ts", "**/*.md"]],
      ["src, ./src/", ["**/src", "**/src/**", "src/**"]],
    ])("sends %j as %j", async (field, expected) => {
      expect(await sentPatterns(field)).toEqual(expected);
    });

    it.each([
      ["*.ts", ["a.ts", "a/b/c.ts"], ["a.tsx", "a/b/c.js"]],
      [
        "src",
        ["src", "src/a.ts", "x/src/deep/b.ts"],
        ["srcs/a.ts", "a/src.ts"],
      ],
      [
        "node_modules",
        ["node_modules/p/index.js", "x/node_modules/p/index.js"],
        ["x/node_modules_old/p.js"],
      ],
      ["dist/", ["dist/a.js", "dist/b/c.js"], ["x/dist/a.js", "distx/a.js"]],
      ["./src", ["src/a.ts", "src/b/c.ts"], ["x/src/a.ts"]],
      ["*.{ts,tsx}", ["a.ts", "b/c.tsx"], ["a.js", "b/c.tsxx"]],
      ["src/**/*.ts", ["src/a.ts", "src/b/c.ts"], ["x/src/a.ts"]],
    ])(
      "makes %j match what VS Code matches",
      async (field, matching, other) => {
        const matches = pathGlobMatcher(await sentPatterns(field));
        for (const path of matching) expect(matches(path), path).toBe(true);
        for (const path of other) expect(matches(path), path).toBe(false);
      },
    );

    it("expands exclude patterns by the same rules", async () => {
      const { search } = await sentFilters("", "node_modules, *.min.js");
      const sent = search.mock.calls[0][0];
      expect(sent.includePatterns).toEqual([]);
      expect(sent.excludePatterns).toEqual([
        "**/node_modules",
        "**/node_modules/**",
        "**/*.min.js",
      ]);
      const hidden = pathGlobMatcher(sent.excludePatterns);
      expect(hidden("node_modules/a/index.js")).toBe(true);
      expect(hidden("packages/x/node_modules/p/index.js")).toBe(true);
      expect(hidden("web/app.min.js")).toBe(true);
      expect(hidden("src/app.js")).toBe(false);
    });

    it.each([
      ["more than 16 patterns", "a, b, c, d, e, f, g, h, i", ""],
      [
        "more than 16 patterns from braces",
        "*.{a,b,c,d,e,f,g,h,i,j,k,l,m,n,o,p,q}",
        "",
      ],
      [
        "more than 2 KiB in total",
        `${"a".repeat(200)}/x`,
        Array.from({ length: 10 }, (_, i) => `${"b".repeat(200)}${i}/x`).join(
          ",",
        ),
      ],
      [
        "17 differently spaced copies of one name before another",
        `{${Array.from({ length: 17 }, (_, i) => `${" ".repeat(i)}a`).join(",")},b}`,
        "",
      ],
      ["20,000 brace groups", "{a}".repeat(20_000), ""],
      ["20,000 brace groups in exclude", "", "{a}".repeat(20_000)],
    ])(
      "refuses an expansion with %s and sends nothing",
      async (_reason, include, exclude) => {
        const { search, state } = await sentFilters(include, exclude);
        expect(search).not.toHaveBeenCalled();
        expect(state).toMatchObject({
          phase: "error",
          response: null,
          error: "workspace Search path filters are invalid",
        });
      },
    );
  });
});

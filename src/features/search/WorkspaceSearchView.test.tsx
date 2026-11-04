import {
  act,
  cleanup,
  createEvent,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useLayoutEffect } from "react";

import { useEditorSettingsStore } from "../../app/editorSettingsStore";
import { pathGlobMatcher } from "../../domain/pathGlobMatcher";
import type { WorkspaceSearchGateway } from "./workspaceSearchDomain";
import { WorkspaceSearchView } from "./WorkspaceSearchView";

const initialSettings = useEditorSettingsStore.getState();

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  useEditorSettingsStore.setState(initialSettings, true);
});

function submitWorkspaceSearch() {
  const form = screen
    .getByRole("searchbox", { name: "Search text" })
    .closest("form");
  expect(form).not.toBeNull();
  fireEvent.submit(form!);
}

describe("Workspace Search view", () => {
  it("retains a query entered before the initial passive effects finish", async () => {
    function SearchWithEarlyInput() {
      // A lazy surface can expose its input before passive effects run.
      useLayoutEffect(() => {
        const input = screen.getByRole("searchbox", { name: "Search text" });
        input.dispatchEvent(
          createEvent.change(input, { target: { value: "needle" } }),
        );
      }, []);
      return (
        <WorkspaceSearchView
          projectId="project-a"
          transitionEpoch={4}
          indexGeneration={0}
          gateway={{ search: vi.fn() }}
          onOpenMatch={vi.fn()}
        />
      );
    }

    await act(async () => {
      render(<SearchWithEarlyInput />);
    });

    expect(screen.getByRole("searchbox", { name: "Search text" })).toHaveValue(
      "needle",
    );
  });

  it("keeps Search filters beside the query instead of adding a details row", () => {
    render(
      <WorkspaceSearchView
        projectId="project-a"
        transitionEpoch={4}
        indexGeneration={0}
        gateway={{ search: vi.fn() }}
        onOpenMatch={vi.fn()}
      />,
    );

    expect(
      screen.getByRole("searchbox", { name: "Search text" }),
    ).toBeVisible();
    expect(screen.getByRole("button", { name: "Match case" })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
    expect(
      screen.getByRole("button", { name: "Match whole word" }),
    ).toHaveAttribute("aria-pressed", "false");
    expect(screen.queryByText("Search details")).toBeNull();
    expect(screen.queryByRole("button", { name: "Search" })).toBeNull();
    expect(screen.queryByLabelText("Files to include")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Match case" }));
    fireEvent.click(screen.getByRole("button", { name: "Match whole word" }));

    expect(screen.getByRole("button", { name: "Match case" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(
      screen.getByRole("button", { name: "Match whole word" }),
    ).toHaveAttribute("aria-pressed", "true");

    fireEvent.click(
      screen.getByRole("button", { name: "Show Search filters" }),
    );

    expect(screen.getByLabelText("Files to include")).toBeVisible();
    expect(screen.getByLabelText("Files to exclude")).toBeVisible();
    expect(
      screen.getByRole("button", { name: "Hide Search filters" }),
    ).toHaveAttribute("aria-expanded", "true");
  });

  it("searches the active Project and opens a canonical result", async () => {
    const gateway: WorkspaceSearchGateway = {
      search: vi.fn().mockResolvedValue({
        requestId: "request-a",
        projectId: "project-a",
        transitionEpoch: 4,
        indexGeneration: 0,
        query: "needle",
        matches: [
          {
            resource: { rootId: "root-b", relativePath: "src/main.ts" },
            lineNumber: 9,
            columnNumber: 3,
            preview: "needle then NEEDLE",
          },
        ],
        roots: [
          {
            rootId: "root-a",
            rootName: "Missing",
            state: "unavailable",
            filesScanned: 0,
            bytesScanned: 0,
            entriesVisited: 0,
            skippedFiles: 0,
            errorCount: 1,
            message: "filesystem root is missing",
          },
          {
            rootId: "root-b",
            rootName: "Sage",
            state: "partial",
            filesScanned: 12,
            bytesScanned: 240,
            entriesVisited: 15,
            skippedFiles: 1,
            errorCount: 0,
            message: null,
          },
        ],
        truncated: true,
        continuationToken: null,
        limitReasons: ["fileSizeLimit"],
        elapsedMs: 7,
      }),
    };
    const open = vi.fn();
    render(
      <WorkspaceSearchView
        projectId="project-a"
        transitionEpoch={4}
        indexGeneration={0}
        gateway={gateway}
        onOpenMatch={open}
      />,
    );

    fireEvent.change(screen.getByLabelText("Search text"), {
      target: { value: "needle" },
    });
    submitWorkspaceSearch();

    const result = await screen.findByRole("button", {
      name: "Open src/main.ts at line 9, column 3",
    });
    expect(screen.getByText("main.ts")).toBeVisible();
    expect(screen.getByText("Sage › src")).toBeVisible();
    expect(screen.getByText("1 match in 1 file")).toBeVisible();
    expect(
      screen.getByRole("button", { name: "Collapse Search results" }),
    ).toBeEnabled();
    expect(result.querySelectorAll("mark")).toHaveLength(1);
    expect(result.querySelector("mark")).toHaveTextContent("needle");
    expect(
      screen.getByText("Some files exceeded the per-file size limit."),
    ).toBeVisible();
    expect(
      screen.getByText("Missing: filesystem root is missing"),
    ).toBeVisible();
    result.focus();
    fireEvent.keyDown(result, { key: "Enter" });
    expect(result).toHaveFocus();
    expect(open).toHaveBeenCalledWith({
      projectId: "project-a",
      transitionEpoch: 4,
      resource: { rootId: "root-b", relativePath: "src/main.ts" },
      lineNumber: 9,
      columnNumber: 3,
      query: "needle",
      caseSensitive: false,
    });
    await waitFor(() => expect(gateway.search).toHaveBeenCalledTimes(1));
  });

  it("projects text-search modes into the native Search request", async () => {
    const search = vi.fn().mockResolvedValue({
      requestId: "request-a",
      projectId: "project-a",
      transitionEpoch: 4,
      indexGeneration: 0,
      query: "needle",
      matches: [],
      roots: [
        {
          rootId: "root-a",
          rootName: "Sage",
          state: "complete" as const,
          filesScanned: 1,
          bytesScanned: 1,
          entriesVisited: 1,
          skippedFiles: 0,
          errorCount: 0,
          message: null,
        },
      ],
      truncated: false,
      continuationToken: null,
      limitReasons: [],
      elapsedMs: 1,
    });
    render(
      <WorkspaceSearchView
        projectId="project-a"
        transitionEpoch={4}
        indexGeneration={0}
        gateway={{ search }}
        onOpenMatch={vi.fn()}
      />,
    );
    fireEvent.change(screen.getByLabelText("Search text"), {
      target: { value: "needle" },
    });
    fireEvent.click(screen.getByLabelText("Match case"));
    fireEvent.click(screen.getByLabelText("Match whole word"));
    submitWorkspaceSearch();

    await waitFor(() =>
      expect(search).toHaveBeenCalledWith(
        expect.objectContaining({
          query: "needle",
          caseSensitive: true,
          wholeWord: true,
        }),
        expect.any(AbortSignal),
      ),
    );
    expect(
      await screen.findByText('No matches found for "needle".'),
    ).toBeVisible();
  });

  it("emphasizes the exact whole-word occurrence represented by each result row", async () => {
    const search = vi.fn().mockResolvedValue({
      requestId: "request-whole-word",
      projectId: "project-a",
      transitionEpoch: 4,
      indexGeneration: 0,
      query: "cat",
      matches: [
        {
          resource: { rootId: "root-a", relativePath: "src/cats.ts" },
          lineNumber: 1,
          columnNumber: 1,
          preview: "cat scatter cat",
        },
        {
          resource: { rootId: "root-a", relativePath: "src/cats.ts" },
          lineNumber: 1,
          columnNumber: 13,
          preview: "cat scatter cat",
        },
      ],
      roots: [
        {
          rootId: "root-a",
          rootName: "Sage",
          state: "complete",
          filesScanned: 1,
          bytesScanned: 15,
          entriesVisited: 1,
          skippedFiles: 0,
          errorCount: 0,
          message: null,
        },
      ],
      truncated: false,
      continuationToken: null,
      limitReasons: [],
      elapsedMs: 1,
    });

    render(
      <WorkspaceSearchView
        projectId="project-a"
        transitionEpoch={4}
        indexGeneration={0}
        gateway={{ search }}
        onOpenMatch={vi.fn()}
      />,
    );

    fireEvent.change(screen.getByRole("searchbox", { name: "Search text" }), {
      target: { value: "cat" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Match whole word" }));
    submitWorkspaceSearch();

    const rows = await screen.findAllByRole("button", {
      name: /Open src\/cats\.ts at line 1, column/,
    });
    expect(rows).toHaveLength(2);
    expect(rows[0].querySelectorAll("mark")).toHaveLength(1);
    expect(rows[0].querySelector("mark")?.previousSibling).toBeNull();
    expect(rows[1].querySelectorAll("mark")).toHaveLength(1);
    expect(rows[1].querySelector("mark")?.nextSibling).toBeNull();
  });

  it("keeps case-insensitive emphasis independent of the browser locale", async () => {
    vi.spyOn(String.prototype, "toLocaleLowerCase").mockImplementation(
      function localeSensitiveLowercase(this: string) {
        return String(this).replaceAll("I", "ı").toLowerCase();
      },
    );
    const search = vi.fn().mockResolvedValue({
      requestId: "request-locale",
      projectId: "project-a",
      transitionEpoch: 4,
      indexGeneration: 0,
      query: "i",
      matches: [
        {
          resource: { rootId: "root-a", relativePath: "src/locale.ts" },
          lineNumber: 1,
          columnNumber: 1,
          preview: "I",
        },
      ],
      roots: [
        {
          rootId: "root-a",
          rootName: "Sage",
          state: "complete",
          filesScanned: 1,
          bytesScanned: 1,
          entriesVisited: 1,
          skippedFiles: 0,
          errorCount: 0,
          message: null,
        },
      ],
      truncated: false,
      continuationToken: null,
      limitReasons: [],
      elapsedMs: 1,
    });

    render(
      <WorkspaceSearchView
        projectId="project-a"
        transitionEpoch={4}
        indexGeneration={0}
        gateway={{ search }}
        onOpenMatch={vi.fn()}
      />,
    );

    fireEvent.change(screen.getByRole("searchbox", { name: "Search text" }), {
      target: { value: "i" },
    });
    submitWorkspaceSearch();

    const row = await screen.findByRole("button", {
      name: "Open src/locale.ts at line 1, column 1",
    });
    expect(row.querySelector("mark")).toHaveTextContent("I");
  });

  it("projects comma-separated include and exclude filters into native Search", async () => {
    const search = vi.fn().mockResolvedValue({
      requestId: "request-a",
      projectId: "project-a",
      transitionEpoch: 4,
      indexGeneration: 0,
      query: "needle",
      matches: [],
      roots: [
        {
          rootId: "root-a",
          rootName: "Sage",
          state: "complete" as const,
          filesScanned: 1,
          bytesScanned: 1,
          entriesVisited: 1,
          skippedFiles: 0,
          errorCount: 0,
          message: null,
        },
      ],
      truncated: false,
      continuationToken: null,
      limitReasons: [],
      elapsedMs: 1,
    });
    render(
      <WorkspaceSearchView
        projectId="project-a"
        transitionEpoch={4}
        indexGeneration={0}
        gateway={{ search }}
        onOpenMatch={vi.fn()}
      />,
    );
    fireEvent.change(screen.getByLabelText("Search text"), {
      target: { value: "needle" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Show Search filters" }),
    );
    fireEvent.change(screen.getByLabelText("Files to include"), {
      target: { value: "src/**/*.ts, tests/**/*.ts" },
    });
    fireEvent.change(screen.getByLabelText("Files to exclude"), {
      target: { value: "**/generated/**" },
    });
    submitWorkspaceSearch();

    await waitFor(() =>
      expect(search).toHaveBeenCalledWith(
        expect.objectContaining({
          includePatterns: ["src/**/*.ts", "tests/**/*.ts"],
          excludePatterns: ["**/generated/**"],
        }),
        expect.any(AbortSignal),
      ),
    );
    expect(
      await screen.findByText('No matches found for "needle".'),
    ).toBeVisible();
  });

  describe("VS Code style include and exclude patterns", () => {
    const projectFiles = [
      "README.md",
      "main.ts",
      "src/app.tsx",
      "src/lib/util.ts",
      "src/lib/util.js",
      "packages/web/src/index.ts",
      "node_modules/p/index.ts",
      "packages/web/node_modules/q/index.ts",
      "dist/bundle.ts",
    ];

    /** A gateway that filters sample files with the real matcher, as native Search would. */
    function filteringGateway() {
      return vi.fn(
        async (request: {
          query: string;
          includePatterns: string[];
          excludePatterns: string[];
        }) => {
          const include = pathGlobMatcher(request.includePatterns);
          const exclude = pathGlobMatcher(request.excludePatterns);
          const kept = projectFiles.filter(
            (path) =>
              (request.includePatterns.length === 0 || include(path)) &&
              !exclude(path),
          );
          return {
            requestId: "request-a",
            projectId: "project-a",
            transitionEpoch: 4,
            indexGeneration: 0,
            query: request.query,
            matches: kept.map((relativePath) => ({
              resource: { rootId: "root-a", relativePath },
              lineNumber: 1,
              columnNumber: 1,
              preview: request.query,
            })),
            roots: [
              {
                rootId: "root-a",
                rootName: "Sage",
                state: "complete" as const,
                filesScanned: kept.length,
                bytesScanned: kept.length,
                entriesVisited: projectFiles.length,
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
        },
      );
    }

    function searchWithFilters(
      search: WorkspaceSearchGateway["search"],
      include: string,
      exclude: string,
    ) {
      render(
        <WorkspaceSearchView
          projectId="project-a"
          transitionEpoch={4}
          indexGeneration={0}
          gateway={{ search }}
          onOpenMatch={vi.fn()}
        />,
      );
      fireEvent.change(screen.getByLabelText("Search text"), {
        target: { value: "needle" },
      });
      fireEvent.click(
        screen.getByRole("button", { name: "Show Search filters" }),
      );
      fireEvent.change(screen.getByLabelText("Files to include"), {
        target: { value: include },
      });
      fireEvent.change(screen.getByLabelText("Files to exclude"), {
        target: { value: exclude },
      });
      submitWorkspaceSearch();
    }

    function shownPaths() {
      return screen
        .queryAllByRole("button", { name: /^Open .* at line 1, column 1$/ })
        .map((row) =>
          row
            .getAttribute("aria-label")!
            .replace(/^Open (.*) at line 1, column 1$/, "$1"),
        )
        .sort();
    }

    it.each([
      [
        "*.{ts,tsx}",
        "node_modules",
        ["**/*.ts", "**/*.tsx"],
        ["**/node_modules", "**/node_modules/**"],
        [
          "dist/bundle.ts",
          "main.ts",
          "packages/web/src/index.ts",
          "src/app.tsx",
          "src/lib/util.ts",
        ],
      ],
      [
        "src",
        "",
        ["**/src", "**/src/**"],
        [],
        [
          "packages/web/src/index.ts",
          "src/app.tsx",
          "src/lib/util.js",
          "src/lib/util.ts",
        ],
      ],
      [
        "./src",
        "*.js",
        ["src", "src/**"],
        ["**/*.js"],
        ["src/app.tsx", "src/lib/util.ts"],
      ],
      [
        "*.ts",
        "dist/, node_modules",
        ["**/*.ts"],
        ["dist/**", "**/node_modules", "**/node_modules/**"],
        ["main.ts", "packages/web/src/index.ts", "src/lib/util.ts"],
      ],
    ])(
      "reads include %j and exclude %j as VS Code does",
      async (include, exclude, includePatterns, excludePatterns, shown) => {
        const search = filteringGateway();
        searchWithFilters(search, include, exclude);

        await waitFor(() => expect(search).toHaveBeenCalledTimes(1));
        expect(search.mock.calls[0][0]).toMatchObject({
          includePatterns,
          excludePatterns,
        });
        await screen.findAllByRole("button", {
          name: /^Open .* at line 1, column 1$/,
        });
        expect(shownPaths()).toEqual(shown);
      },
    );

    it("shows the invalid-filter message and sends nothing when an expansion passes the limits", async () => {
      const search = filteringGateway();
      searchWithFilters(search, "a, b, c, d, e, f, g, h, i", "");

      expect(
        await screen.findByText("workspace Search path filters are invalid"),
      ).toBeVisible();
      expect(search).not.toHaveBeenCalled();
    });

    it("shows the invalid-filter message and sends nothing for a pasted run of brace groups", async () => {
      const search = filteringGateway();
      searchWithFilters(search, "{a}".repeat(20_000), "");

      expect(
        await screen.findByText("workspace Search path filters are invalid"),
      ).toBeVisible();
      expect(search).not.toHaveBeenCalled();
    });
  });

  it("asks native Search to keep ignored files only when the setting is off", async () => {
    const search = vi.fn().mockRejectedValue(new Error("stop"));
    render(
      <WorkspaceSearchView
        projectId="project-a"
        transitionEpoch={4}
        indexGeneration={0}
        gateway={{ search }}
        onOpenMatch={vi.fn()}
      />,
    );
    fireEvent.change(screen.getByLabelText("Search text"), {
      target: { value: "needle" },
    });
    submitWorkspaceSearch();
    await waitFor(() => expect(search).toHaveBeenCalledTimes(1));
    expect(search.mock.calls[0][0]).not.toHaveProperty("useIgnoreFiles");
    expect(await screen.findByText("stop")).toBeVisible();

    act(() => {
      useEditorSettingsStore.setState({
        settings: {
          ...useEditorSettingsStore.getState().settings,
          "search.useIgnoreFiles": false,
        },
      });
    });
    // The setting retires the shown result like any other criterion.
    expect(screen.queryByText("stop")).toBeNull();
    submitWorkspaceSearch();
    await waitFor(() => expect(search).toHaveBeenCalledTimes(2));
    expect(search.mock.calls[1][0]).toMatchObject({ useIgnoreFiles: false });
  });

  it("sends files.exclude only when it differs from the default and retires results when it changes", async () => {
    const search = vi.fn().mockRejectedValue(new Error("stop"));
    render(
      <WorkspaceSearchView
        projectId="project-a"
        transitionEpoch={4}
        indexGeneration={0}
        gateway={{ search }}
        onOpenMatch={vi.fn()}
      />,
    );
    fireEvent.change(screen.getByLabelText("Search text"), {
      target: { value: "needle" },
    });
    submitWorkspaceSearch();
    await waitFor(() => expect(search).toHaveBeenCalledTimes(1));
    expect(search.mock.calls[0][0]).not.toHaveProperty("filesExclude");
    expect(await screen.findByText("stop")).toBeVisible();

    act(() => {
      useEditorSettingsStore.setState({
        settings: {
          ...useEditorSettingsStore.getState().settings,
          "files.exclude": ["**/dist"],
        },
      });
    });
    expect(screen.queryByText("stop")).toBeNull();
    submitWorkspaceSearch();
    await waitFor(() => expect(search).toHaveBeenCalledTimes(2));
    expect(search.mock.calls[1][0]).toMatchObject({
      filesExclude: ["**/dist"],
    });
    expect(await screen.findByText("stop")).toBeVisible();

    // An equal list with a new identity is no change.
    act(() => {
      useEditorSettingsStore.setState({
        settings: {
          ...useEditorSettingsStore.getState().settings,
          "files.exclude": ["**/dist"],
        },
      });
    });
    expect(screen.getByText("stop")).toBeVisible();
  });

  it("refreshes the current request and collapses a file group without losing it", async () => {
    const response = {
      requestId: "request-a",
      projectId: "project-a",
      transitionEpoch: 4,
      indexGeneration: 0,
      query: "needle",
      matches: [
        {
          resource: { rootId: "root-a", relativePath: "src/long/path.ts" },
          lineNumber: 1,
          columnNumber: 1,
          preview: "needle",
        },
      ],
      roots: [
        {
          rootId: "root-a",
          rootName: "Sage",
          state: "complete" as const,
          filesScanned: 1,
          bytesScanned: 6,
          entriesVisited: 1,
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
    const search = vi.fn().mockResolvedValue(response);
    render(
      <WorkspaceSearchView
        projectId="project-a"
        transitionEpoch={4}
        indexGeneration={0}
        gateway={{ search }}
        onOpenMatch={vi.fn()}
      />,
    );
    fireEvent.change(screen.getByLabelText("Search text"), {
      target: { value: "needle" },
    });
    submitWorkspaceSearch();
    await screen.findByRole("button", {
      name: "Collapse results for src/long/path.ts",
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Collapse Search results" }),
    );
    expect(
      screen.queryByRole("button", {
        name: "Open src/long/path.ts at line 1, column 1",
      }),
    ).toBeNull();
    fireEvent.click(
      screen.getByRole("button", { name: "Expand Search results" }),
    );
    expect(
      screen.getByRole("button", {
        name: "Open src/long/path.ts at line 1, column 1",
      }),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Refresh Search" }));
    await waitFor(() => expect(search).toHaveBeenCalledTimes(2));
  });

  it("keeps search unavailable when no Project is open", () => {
    render(
      <WorkspaceSearchView
        projectId={null}
        transitionEpoch={0}
        indexGeneration={0}
        gateway={{ search: vi.fn() }}
        onOpenMatch={vi.fn()}
      />,
    );

    expect(
      screen.getByRole("searchbox", { name: "Search text" }),
    ).toBeDisabled();
    expect(
      screen.getByText("Open a Project to search its files."),
    ).toBeVisible();
  });

  it("shows Continue search while a token remains under the page cap", async () => {
    const token =
      '{"v":1,"g":0,"e":4,"c":0,"w":0,"f":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","q":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","r":"root-a","p":"safe.txt","l":1,"o":1}';
    const gateway: WorkspaceSearchGateway = {
      search: vi.fn().mockResolvedValue({
        requestId: "request-a",
        projectId: "project-a",
        transitionEpoch: 4,
        indexGeneration: 0,
        query: "needle",
        matches: [
          {
            resource: { rootId: "root-a", relativePath: "safe.txt" },
            lineNumber: 1,
            columnNumber: 1,
            preview: "needle",
          },
        ],
        roots: [
          {
            rootId: "root-a",
            rootName: "Sage",
            state: "partial",
            filesScanned: 1,
            bytesScanned: 6,
            entriesVisited: 1,
            skippedFiles: 1,
            errorCount: 0,
            message: null,
          },
        ],
        truncated: true,
        continuationToken: token,
        limitReasons: ["fileSizeLimit"],
        elapsedMs: 4,
      }),
    };
    render(
      <WorkspaceSearchView
        projectId="project-a"
        transitionEpoch={4}
        indexGeneration={0}
        gateway={gateway}
        onOpenMatch={vi.fn()}
      />,
    );
    fireEvent.change(screen.getByLabelText("Search text"), {
      target: { value: "needle" },
    });
    submitWorkspaceSearch();
    expect(
      await screen.findByRole("button", { name: "Continue search" }),
    ).toBeVisible();
  });
});

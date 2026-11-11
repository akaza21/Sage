import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import QuickOpen from "./QuickOpen";
import { setQuickOpenInvoke } from "./quickOpenGateway";
import { noteRecentFile } from "./quickOpenState";

const initialState = useWorkbenchStore.getState();
const FILES = ["src/app.ts", "src/apple/core.ts", "docs/api.md", "README.md"];

interface Sent {
  query: string;
  generation: number;
  session: number;
  recent: Array<{ relativePath: string }>;
}

function fakeNative(
  options: {
    refreshingFor?: number;
    delayMs?: number;
    beforeResponse?: (query: string) => Promise<void>;
    supersededFor?: number;
    files?: () => string[];
  } = {},
) {
  const sent: Sent[] = [];
  let refreshing = options.refreshingFor ?? 0;
  let superseded = options.supersededFor ?? 0;
  setQuickOpenInvoke(async (command, args) => {
    expect(command).toBe("quick_open_query");
    const request = args.request as Sent;
    sent.push(request);
    await options.beforeResponse?.(request.query);
    if (options.delayMs) {
      await new Promise((resolve) => setTimeout(resolve, options.delayMs));
    }
    const letters = request.query.toLowerCase().replace(/\s/g, "");
    if (letters !== "" && superseded-- > 0) {
      return {
        state: "superseded",
        results: [],
        indexedFiles: 0,
        truncated: false,
        built: false,
        refreshing: false,
        indexBytes: 0,
        readyMs: 0,
      };
    }
    const results = (options.files?.() ?? FILES)
      .filter((path) => path.toLowerCase().includes(letters))
      .map((relativePath) => ({
        rootId: "root-a",
        relativePath,
        positions: letters ? [relativePath.toLowerCase().indexOf(letters)] : [],
      }));
    return {
      state: "ready",
      results: letters ? results : [],
      indexedFiles: FILES.length,
      truncated: false,
      built: false,
      refreshing: letters !== "" && refreshing-- > 0,
      indexBytes: 0,
      readyMs: 0,
    };
  });
  return sent;
}

const openFile = vi.fn(async () => {});
const openFileLocation = vi.fn(async () => {});

beforeEach(() => {
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "A", roots: [] },
    nativeTransitionEpoch: 3,
    openFile,
    openFileLocation,
  });
});

afterEach(() => {
  cleanup();
  setQuickOpenInvoke(null);
  useWorkbenchStore.setState(initialState, true);
  vi.clearAllMocks();
  vi.useRealTimers();
  Reflect.deleteProperty(Element.prototype, "scrollIntoView");
});

const input = () =>
  screen.getByRole("combobox", { name: "Search files by name" });
const options = () =>
  screen.queryAllByRole("option").map((option) => option.textContent);

describe("Quick Open", () => {
  it("lists matches, moves with the arrows, and opens the chosen File", async () => {
    const sent = fakeNative();
    const onClose = vi.fn();
    noteRecentFile("project-a", {
      rootId: "root-a",
      relativePath: "docs/api.md",
    });
    render(<QuickOpen onClose={onClose} />);
    expect(input()).toHaveFocus();
    fireEvent.change(input(), { target: { value: "app" } });
    await waitFor(() =>
      expect(options()).toEqual(["app.tssrc", "core.tssrc/apple"]),
    );
    const last = sent.at(-1)!;
    expect(last).toMatchObject({
      query: "app",
      projectId: "project-a",
      expectedTransitionEpoch: 3,
      limit: 50,
    });
    expect(last.recent).toEqual([
      { rootId: "root-a", relativePath: "docs/api.md" },
    ]);
    // One session per opening; every query a newer generation.
    expect(new Set(sent.map((request) => request.session)).size).toBe(1);
    expect(sent.at(-1)!.generation).toBeGreaterThan(sent[0].generation);
    expect(screen.getAllByRole("option")[0]).toHaveAttribute(
      "aria-selected",
      "true",
    );

    const scrollIntoView = vi.fn();
    Object.defineProperty(Element.prototype, "scrollIntoView", {
      configurable: true,
      value: scrollIntoView,
    });
    fireEvent.keyDown(input(), { key: "ArrowDown" });
    expect(screen.getAllByRole("option")[1]).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(scrollIntoView).toHaveBeenLastCalledWith({ block: "nearest" });
    fireEvent.keyDown(input(), { key: "ArrowDown" });
    expect(screen.getAllByRole("option")[0]).toHaveAttribute(
      "aria-selected",
      "true",
    );
    fireEvent.keyDown(input(), { key: "ArrowUp" });
    fireEvent.keyDown(input(), { key: "Enter" });
    expect(onClose).toHaveBeenCalledOnce();
    expect(openFile).toHaveBeenCalledWith({
      rootId: "root-a",
      relativePath: "src/apple/core.ts",
    });
  });

  it("goes to the line and column a :line:column suffix names", async () => {
    fakeNative();
    render(<QuickOpen onClose={vi.fn()} />);
    fireEvent.change(input(), { target: { value: "readme:12:4" } });
    await waitFor(() => expect(options()).toEqual(["README.md"]));
    fireEvent.keyDown(input(), { key: "Enter" });
    expect(openFileLocation).toHaveBeenCalledWith({
      projectId: "project-a",
      transitionEpoch: 3,
      resource: { rootId: "root-a", relativePath: "README.md" },
      lineNumber: 12,
      columnNumber: 4,
      selection: { kind: "caret" },
    });
    expect(openFile).not.toHaveBeenCalled();
  });

  it("goes to a line of the active File for a bare :line", async () => {
    useWorkbenchStore.setState({
      tabs: [
        {
          id: "file:a",
          kind: "file",
          title: "a.ts",
          position: 0,
          pinned: false,
          dirty: false,
          resource: { rootId: "root-a", relativePath: "src/a.ts" },
        },
      ],
      activeTabId: "file:a",
    });
    const sent = fakeNative();
    render(<QuickOpen onClose={vi.fn()} />);
    await waitFor(() => expect(sent).toHaveLength(1));
    fireEvent.change(input(), { target: { value: ":30" } });
    await screen.findByText("Press Enter to go to line 30 in the active File.");
    // Nothing is asked for a bare line; the opening query was the only one.
    expect(sent.map((request) => request.query)).toEqual([""]);
    fireEvent.keyDown(input(), { key: "Enter" });
    expect(openFileLocation).toHaveBeenCalledWith(
      expect.objectContaining({
        resource: { rootId: "root-a", relativePath: "src/a.ts" },
        lineNumber: 30,
        columnNumber: 1,
      }),
    );
  });

  it("explains a slow first answer as Indexing, then shows no-match text", async () => {
    fakeNative({ delayMs: 400 });
    render(<QuickOpen onClose={vi.fn()} />);
    fireEvent.change(input(), { target: { value: "zzz" } });
    expect(await screen.findByText("Indexing…")).toBeInTheDocument();
    expect(
      await screen.findByText("No matching files.", {}, { timeout: 2000 }),
    ).toBeInTheDocument();
  });

  it("asks again while the index is refreshing, then stops", async () => {
    const sent = fakeNative({ refreshingFor: 2 });
    render(<QuickOpen onClose={vi.fn()} />);
    fireEvent.change(input(), { target: { value: "api" } });
    await waitFor(
      () =>
        expect(sent.filter((request) => request.query === "api")).toHaveLength(
          3,
        ),
      {
        timeout: 2000,
      },
    );
    await act(() => new Promise((resolve) => setTimeout(resolve, 400)));
    expect(sent.filter((request) => request.query === "api")).toHaveLength(3);
  });

  it("waits for the current query's answer before Enter opens a File", async () => {
    fakeNative({ delayMs: 200 });
    render(<QuickOpen onClose={vi.fn()} />);
    fireEvent.change(input(), { target: { value: "app" } });
    await waitFor(() => expect(options()).toHaveLength(2), { timeout: 2000 });
    // Typed and confirmed before "readme"'s answer arrives.
    fireEvent.change(input(), { target: { value: "readme" } });
    fireEvent.keyDown(input(), { key: "Enter" });
    expect(openFile).not.toHaveBeenCalled();
    await waitFor(
      () =>
        expect(openFile).toHaveBeenCalledWith({
          rootId: "root-a",
          relativePath: "README.md",
        }),
      { timeout: 2000 },
    );
    expect(openFile).toHaveBeenCalledOnce();
  });

  it.each(["click", "Enter"])(
    "waits for fresh results when %s confirms a stale row",
    async (confirmation) => {
      let releaseResponse!: () => void;
      const responseReady = new Promise<void>((resolve) => {
        releaseResponse = resolve;
      });
      fakeNative({
        beforeResponse: async (query) => {
          if (query === "readme") await responseReady;
        },
      });
      const openedFiles: string[] = [];
      useWorkbenchStore.setState({
        openFile: async (resource) => {
          openedFiles.push(resource.relativePath);
        },
      });
      render(<QuickOpen onClose={() => {}} />);
      fireEvent.change(input(), { target: { value: "app" } });
      await waitFor(() =>
        expect(options()).toEqual(["app.tssrc", "core.tssrc/apple"]),
      );
      fireEvent.change(input(), { target: { value: "readme" } });
      if (confirmation === "click") {
        fireEvent.click(screen.getByRole("option", { name: /core.ts/ }));
      } else {
        fireEvent.keyDown(input(), { key: "Enter" });
      }
      expect(openedFiles).toEqual([]);
      await act(async () => {
        releaseResponse();
        await responseReady;
      });
      await waitFor(() => expect(openedFiles).toEqual(["README.md"]));
    },
  );

  it("opens the clicked File when the results match the current query", async () => {
    fakeNative();
    const openedFiles: string[] = [];
    useWorkbenchStore.setState({
      openFile: async (resource) => {
        openedFiles.push(resource.relativePath);
      },
    });
    render(<QuickOpen onClose={() => {}} />);
    fireEvent.change(input(), { target: { value: "app" } });
    await waitFor(() =>
      expect(options()).toEqual(["app.tssrc", "core.tssrc/apple"]),
    );
    fireEvent.click(screen.getByRole("option", { name: /core.ts/ }));
    expect(openedFiles).toEqual(["src/apple/core.ts"]);
  });

  it("asks once more after an interruption, then says so", async () => {
    const sent = fakeNative({ supersededFor: 1 });
    const { unmount } = render(<QuickOpen onClose={vi.fn()} />);
    fireEvent.change(input(), { target: { value: "api" } });
    await waitFor(() => expect(options()).toEqual(["api.mddocs"]));
    expect(sent.filter((request) => request.query === "api")).toHaveLength(2);
    unmount();

    fakeNative({ supersededFor: 2, delayMs: 100 });
    render(<QuickOpen onClose={vi.fn()} />);
    fireEvent.change(input(), { target: { value: "api" } });
    await waitFor(
      () =>
        expect(screen.getByRole("status")).toHaveTextContent(
          "Quick Open was interrupted. Type to try again.",
        ),
      { timeout: 2000 },
    );
    // The failure belongs to "api": a new query clears it, and Enter
    // waits for that query's answer.
    fireEvent.change(input(), { target: { value: "readme" } });
    expect(screen.getByRole("status")).not.toHaveTextContent("interrupted");
    fireEvent.keyDown(input(), { key: "Enter" });
    await waitFor(
      () =>
        expect(openFile).toHaveBeenCalledWith({
          rootId: "root-a",
          relativePath: "README.md",
        }),
      { timeout: 2000 },
    );
  });

  it("keeps the chosen File selected when a refreshed answer reorders the list", async () => {
    let files = ["src/app.ts", "src/apple/core.ts"];
    fakeNative({ refreshingFor: 1, files: () => files });
    render(<QuickOpen onClose={vi.fn()} />);
    fireEvent.change(input(), { target: { value: "app" } });
    await waitFor(() => expect(options()).toHaveLength(2));
    fireEvent.keyDown(input(), { key: "ArrowDown" });
    files = ["lib/app.ts", "src/apple/core.ts", "src/app.ts"];
    await waitFor(() => expect(options()).toHaveLength(3), { timeout: 2000 });
    expect(screen.getAllByRole("option")[1]).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getAllByRole("option")[1]).toHaveTextContent("core.ts");
    // Typing a :line suffix keeps it too.
    fireEvent.change(input(), { target: { value: "app:" } });
    fireEvent.change(input(), { target: { value: "app:9" } });
    expect(screen.getAllByRole("option")[1]).toHaveAttribute(
      "aria-selected",
      "true",
    );
    fireEvent.keyDown(input(), { key: "Enter" });
    expect(openFileLocation).toHaveBeenCalledWith(
      expect.objectContaining({
        resource: { rootId: "root-a", relativePath: "src/apple/core.ts" },
        lineNumber: 9,
      }),
    );
  });

  it("ignores the Enter that ends an IME composition", async () => {
    fakeNative();
    render(<QuickOpen onClose={vi.fn()} />);
    fireEvent.change(input(), { target: { value: "readme" } });
    await waitFor(() => expect(options()).toEqual(["README.md"]));
    fireEvent.keyDown(input(), { key: "Enter", keyCode: 229 });
    fireEvent.keyDown(input(), { key: "Enter", isComposing: true });
    expect(openFile).not.toHaveBeenCalled();
    fireEvent.keyDown(input(), { key: "Enter" });
    expect(openFile).toHaveBeenCalledOnce();
  });

  it("keeps the focus in the input when the list is clicked", async () => {
    fakeNative();
    render(<QuickOpen onClose={vi.fn()} />);
    const dialog = screen.getByRole("dialog", { name: "Quick Open" });
    const list = screen.getByRole("listbox");
    expect(fireEvent.mouseDown(list)).toBe(false);
    expect(fireEvent.mouseDown(input())).toBe(true);
    expect(dialog).toBeInTheDocument();
  });

  it("closes on Escape and returns focus", async () => {
    fakeNative();
    const onClose = vi.fn();
    const before = document.createElement("button");
    document.body.append(before);
    before.focus();
    render(<QuickOpen onClose={onClose} />);
    fireEvent.keyDown(input(), { key: "Escape" });
    expect(onClose).toHaveBeenCalledOnce();
    expect(before).toHaveFocus();
    before.remove();
  });

  it("shows why it can't run without a Project or outside the desktop app", async () => {
    useWorkbenchStore.setState({ project: null });
    const { unmount } = render(<QuickOpen onClose={vi.fn()} />);
    expect(screen.getByRole("status")).toHaveTextContent(
      "Open a project to find its files.",
    );
    unmount();
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
    });
    render(<QuickOpen onClose={vi.fn()} />);
    expect(await screen.findByRole("status")).toHaveTextContent(
      "Quick Open is available in the Sage desktop runtime.",
    );
  });
});

describe("Quick Open's commands (ED-2a)", () => {
  it("lists commands for >, filters them by word, and runs the chosen one", async () => {
    const sent = fakeNative();
    const setSidebarView = vi.fn();
    const onClose = vi.fn();
    useWorkbenchStore.setState({ setSidebarView });
    render(<QuickOpen seed=">" onClose={onClose} />);
    const box = screen.getByRole("combobox", { name: "Run a command" });
    expect(box).toHaveValue(">");
    expect(options()).toContain("Show Search");
    expect(options()).toContain("Go to File…Ctrl+P");

    fireEvent.change(box, { target: { value: ">search show" } });
    expect(options()).toEqual(["Show Search"]);
    fireEvent.keyDown(box, { key: "Enter" });
    expect(setSidebarView).toHaveBeenCalledWith("search");
    expect(onClose).toHaveBeenCalledOnce();
    // Commands never ask the File index.
    await act(() => new Promise((resolve) => setTimeout(resolve, 50)));
    expect(sent).toEqual([]);
  });

  it("gives the focus back when a command doesn't take it", async () => {
    fakeNative();
    const before = document.createElement("button");
    document.body.append(before);
    before.focus();
    useWorkbenchStore.setState({ setSidebarView: vi.fn() });
    const { unmount } = render(
      <QuickOpen seed=">show search" onClose={vi.fn()} />,
    );
    const box = screen.getByRole("combobox", { name: "Run a command" });
    expect(box).toHaveFocus();
    fireEvent.keyDown(box, { key: "Enter" });
    // The box would unmount here, leaving the focus nowhere.
    unmount();
    expect(document.activeElement).toBe(document.body);
    await waitFor(() => expect(before).toHaveFocus());
    before.remove();
  });

  it("switches between Files and commands with the chords that open it", () => {
    fakeNative();
    render(<QuickOpen onClose={vi.fn()} />);
    fireEvent.keyDown(input(), { key: "P", ctrlKey: true, shiftKey: true });
    const box = screen.getByRole("combobox", { name: "Run a command" });
    expect(box).toHaveValue(">");
    fireEvent.keyDown(box, { key: "p", ctrlKey: true });
    expect(input()).toHaveValue("");
  });

  it("goes back to Files for Go to File, and says when nothing matches", async () => {
    fakeNative();
    const onClose = vi.fn();
    render(<QuickOpen seed=">" onClose={onClose} />);
    const box = screen.getByRole("combobox", { name: "Run a command" });
    fireEvent.change(box, { target: { value: ">zzz" } });
    expect(screen.getByRole("status")).toHaveTextContent(
      "No matching commands.",
    );
    fireEvent.change(box, { target: { value: ">go to file" } });
    fireEvent.keyDown(box, { key: "Enter" });
    expect(onClose).not.toHaveBeenCalled();
    expect(input()).toHaveValue("");
    fireEvent.change(input(), { target: { value: "app" } });
    await waitFor(() => expect(options()).toHaveLength(2));
  });
});

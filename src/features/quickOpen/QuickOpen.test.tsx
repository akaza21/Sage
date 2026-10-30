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
  options: { refreshingFor?: number; delayMs?: number } = {},
) {
  const sent: Sent[] = [];
  let refreshing = options.refreshingFor ?? 0;
  setQuickOpenInvoke(async (command, args) => {
    expect(command).toBe("quick_open_query");
    const request = args.request as Sent;
    sent.push(request);
    if (options.delayMs) {
      await new Promise((resolve) => setTimeout(resolve, options.delayMs));
    }
    const letters = request.query.toLowerCase().replace(/\s/g, "");
    const results = FILES.filter((path) =>
      path.toLowerCase().includes(letters),
    ).map((relativePath) => ({
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
});

const input = () =>
  screen.getByRole("combobox", { name: "Search Files by name" });
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

    fireEvent.keyDown(input(), { key: "ArrowDown" });
    expect(screen.getAllByRole("option")[1]).toHaveAttribute(
      "aria-selected",
      "true",
    );
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
    fakeNative();
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
    render(<QuickOpen onClose={vi.fn()} />);
    fireEvent.change(input(), { target: { value: ":30" } });
    await screen.findByText("Press Enter to go to line 30 in the active File.");
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
      await screen.findByText("No matching Files.", {}, { timeout: 2000 }),
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
      "Open a Project to find its Files.",
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

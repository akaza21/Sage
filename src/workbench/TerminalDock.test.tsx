import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useState } from "react";
import { useWorkbenchStore } from "../app/store";
import {
  clearAgentChatOwner,
  pinAgentChatOwner,
  pinnedAgentChatOwner,
} from "../app/agentChatOwner";

const native = vi.hoisted(() => ({
  create: vi.fn(),
  read: vi.fn(),
  close: vi.fn(),
  resize: vi.fn(),
  write: vi.fn(),
}));
const xterm = vi.hoisted(() => ({
  refresh: vi.fn(),
  data: null as ((data: string) => void) | null,
}));
let resizeCallback: (() => void) | null = null;

vi.mock("../services/userTerminalDesktop", () => ({
  createUserTerminal: native.create,
  readUserTerminal: native.read,
  closeUserTerminal: native.close,
  resizeUserTerminal: native.resize,
  writeUserTerminal: native.write,
}));

vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    cols = 1;
    rows = 1;
    open() {}
    resize(columns: number, rows: number) {
      this.cols = columns;
      this.rows = rows;
    }
    write(_bytes: Uint8Array | string, done?: () => void) {
      done?.();
    }
    focus() {}
    refresh(start: number, end: number) {
      xterm.refresh(start, end);
    }
    dispose() {}
    onData(callback: (data: string) => void) {
      xterm.data = callback;
      return { dispose() {} };
    }
  },
}));

import { TerminalDock, TerminalDockForWorkbench } from "./TerminalDock";

const initialWorkbenchState = useWorkbenchStore.getState();

const scope = {
  projectId: "project-1",
  projectGeneration: "2",
  chatId: "chat-1",
};
const handle = {
  sessionId: "f99cd56a-32d9-4600-a1f6-269e05734268",
  revision: "1",
  shellPath: "C:\\Windows\\System32\\cmd.exe",
};

beforeEach(() => {
  Object.values(native).forEach((mock) => mock.mockReset());
  native.create.mockResolvedValue(handle);
  native.read.mockImplementation(() => new Promise(() => undefined));
  native.close.mockResolvedValue(undefined);
  native.resize.mockResolvedValue(undefined);
  xterm.refresh.mockReset();
  xterm.data = null;
  resizeCallback = null;
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(callback: () => void) {
        resizeCallback = callback;
      }
      observe() {}
      disconnect() {}
    },
  );
});

it("delivers a zero-delay typing burst without losing characters", async () => {
  let settleFirst: (() => void) | undefined;
  native.write
    .mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          settleFirst = resolve;
        }),
    )
    .mockResolvedValue(undefined);
  render(<TerminalDock scope={scope} onClose={() => undefined} />);
  await screen.findByTitle(handle.shellPath);
  const burst = "echo CP9_NATIVE_TERMINAL_OK\r";
  act(() => {
    for (const character of burst) xterm.data?.(character);
  });
  await waitFor(() => expect(native.write).toHaveBeenCalled());
  settleFirst?.();
  await waitFor(() =>
    expect(native.write.mock.calls.map((call) => call[2]).join("")).toBe(burst),
  );
  expect(screen.queryByText(/input.*not sent/i)).not.toBeInTheDocument();
});

it("shows session ended and ignores later input when the shell has exited", async () => {
  native.write.mockRejectedValue({ code: "ended" });
  render(<TerminalDock scope={scope} onClose={() => undefined} />);
  await screen.findByTitle(handle.shellPath);
  act(() => xterm.data?.("x"));
  expect(await screen.findByText("Session ended")).toBeInTheDocument();
  act(() => xterm.data?.("y"));
  expect(native.write).toHaveBeenCalledTimes(1);
});

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialWorkbenchState, true);
  clearAgentChatOwner();
  vi.unstubAllGlobals();
});

it("creates a scoped PTY and waits for native close before dismissing", async () => {
  const onClose = vi.fn();
  render(<TerminalDock scope={scope} onClose={onClose} />);
  expect(await screen.findByTitle(handle.shellPath)).toBeInTheDocument();
  expect(native.create).toHaveBeenCalledWith(scope, 1, 1);
  fireEvent.click(screen.getByRole("button", { name: "Close terminal" }));
  await waitFor(() => expect(native.close).toHaveBeenCalledWith(scope, handle));
  await waitFor(() => expect(onClose).toHaveBeenCalledOnce());
});

it("closes a creation that finishes after the dock unmounts", async () => {
  let resolveCreate: ((value: typeof handle) => void) | undefined;
  native.create.mockImplementation(
    () =>
      new Promise((resolve) => {
        resolveCreate = resolve;
      }),
  );
  const view = render(<TerminalDock scope={scope} onClose={() => undefined} />);
  view.unmount();
  resolveCreate?.(handle);
  await waitFor(() => expect(native.close).toHaveBeenCalledWith(scope, handle));
});

it("resizes for pane changes and refreshes after a DPI/window change", async () => {
  render(<TerminalDock scope={scope} onClose={() => undefined} />);
  await screen.findByTitle(handle.shellPath);
  const viewport = document.querySelector(".terminal-dock__viewport");
  expect(viewport).not.toBeNull();
  Object.defineProperty(viewport, "clientWidth", {
    configurable: true,
    value: 730,
  });
  Object.defineProperty(viewport, "clientHeight", {
    configurable: true,
    value: 160,
  });
  resizeCallback?.();
  await waitFor(() =>
    expect(native.resize).toHaveBeenCalledWith(scope, handle, 100, 10),
  );
  const before = window.devicePixelRatio;
  Object.defineProperty(window, "devicePixelRatio", {
    configurable: true,
    value: before + 1,
  });
  fireEvent(window, new Event("resize"));
  expect(xterm.refresh).toHaveBeenCalledWith(0, 9);
  Object.defineProperty(window, "devicePixelRatio", {
    configurable: true,
    value: before,
  });
});

it("closes the dock and its owned session when the focused chat changes", async () => {
  useWorkbenchStore.setState({
    project: { id: scope.projectId, name: "Project", roots: [] },
    projectGeneration: 2,
    nativeTransitionEpoch: 2,
    projectTransitioning: false,
    activeTabId: scope.chatId,
    tabs: [
      {
        id: scope.chatId,
        kind: "chat",
        title: "First",
        position: 0,
        pinned: false,
        dirty: false,
      },
      {
        id: "chat-2",
        kind: "chat",
        title: "Second",
        position: 1,
        pinned: false,
        dirty: false,
      },
    ],
  });
  function Host() {
    const [open, setOpen] = useState(true);
    return open ? (
      <TerminalDockForWorkbench setOpen={setOpen} pinnedChatId={null} />
    ) : null;
  }
  render(<Host />);
  await screen.findByTitle(handle.shellPath);
  useWorkbenchStore.setState({ activeTabId: "chat-2" });
  await waitFor(() => expect(native.close).toHaveBeenCalledWith(scope, handle));
});

it("opens for a valid project when generation and transition epoch differ", async () => {
  useWorkbenchStore.setState({
    project: { id: scope.projectId, name: "Project", roots: [] },
    projectGeneration: 7,
    nativeTransitionEpoch: 0,
    projectTransitioning: false,
    activeTabId: scope.chatId,
    tabs: [
      {
        id: scope.chatId,
        kind: "chat",
        title: "First",
        position: 0,
        pinned: false,
        dirty: false,
      },
    ],
  });
  render(
    <TerminalDockForWorkbench setOpen={() => undefined} pinnedChatId={null} />,
  );
  await screen.findByTitle(handle.shellPath);
  expect(native.create).toHaveBeenCalledWith(
    { ...scope, projectGeneration: "0" },
    1,
    1,
  );
});

it("closes when the UI project generation changes without a native epoch change", async () => {
  useWorkbenchStore.setState({
    project: { id: scope.projectId, name: "Project", roots: [] },
    projectGeneration: 7,
    nativeTransitionEpoch: 0,
    projectTransitioning: false,
    activeTabId: scope.chatId,
    tabs: [
      {
        id: scope.chatId,
        kind: "chat",
        title: "First",
        position: 0,
        pinned: false,
        dirty: false,
      },
    ],
  });
  function Host() {
    const [open, setOpen] = useState(true);
    return open ? (
      <TerminalDockForWorkbench setOpen={setOpen} pinnedChatId={null} />
    ) : null;
  }
  render(<Host />);
  await screen.findByTitle(handle.shellPath);
  useWorkbenchStore.setState({ projectGeneration: 8 });
  await waitFor(() =>
    expect(native.close).toHaveBeenCalledWith(
      { ...scope, projectGeneration: "0" },
      handle,
    ),
  );
});

it("opens for the pinned visible chat when a shared file tab is active", async () => {
  pinAgentChatOwner("chat-2");
  useWorkbenchStore.setState({
    project: { id: scope.projectId, name: "Project", roots: [] },
    projectGeneration: 2,
    nativeTransitionEpoch: 0,
    projectTransitioning: false,
    activeTabId: "file-1",
    tabs: [
      {
        id: scope.chatId,
        kind: "chat",
        title: "First",
        position: 0,
        pinned: false,
        dirty: false,
      },
      {
        id: "chat-2",
        kind: "chat",
        title: "Second",
        position: 1,
        pinned: false,
        dirty: false,
      },
      {
        id: "file-1",
        kind: "file",
        title: "one.ts",
        position: 2,
        pinned: false,
        dirty: false,
        resource: { rootId: "root-1", relativePath: "one.ts" },
      },
    ],
  });
  render(
    <TerminalDockForWorkbench
      setOpen={() => undefined}
      pinnedChatId={pinnedAgentChatOwner()}
    />,
  );
  await screen.findByTitle(handle.shellPath);
  expect(native.create).toHaveBeenCalledWith(
    { ...scope, projectGeneration: "0", chatId: "chat-2" },
    1,
    1,
  );
});

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
  terminalErrorLabel: (error: { code?: string }) =>
    error.code === "ended" ? "Session ended" : "Terminal operation failed.",
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

import { TerminalDock } from "./TerminalDock";
import { TerminalDockForWorkbench } from "./TerminalDockShell";

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
  native.write.mockResolvedValue(undefined);
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

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialWorkbenchState, true);
  vi.unstubAllGlobals();
});

it("delivers a zero-delay typing burst without losing characters", async () => {
  let settleFirst: (() => void) | undefined;
  native.write.mockImplementationOnce(
    () =>
      new Promise<void>((resolve) => {
        settleFirst = resolve;
      }),
  );
  const onStatus = vi.fn();
  render(
    <TerminalDock
      scope={scope}
      session={handle}
      ended={false}
      onStatus={onStatus}
    />,
  );
  await waitFor(() => expect(xterm.data).not.toBeNull());
  const burst = "echo CP9_NATIVE_TERMINAL_OK\r";
  act(() => {
    for (const character of burst) xterm.data?.(character);
  });
  await waitFor(() => expect(native.write).toHaveBeenCalled());
  settleFirst?.();
  await waitFor(() =>
    expect(native.write.mock.calls.map((call) => call[2]).join("")).toBe(burst),
  );
  expect(onStatus).not.toHaveBeenCalled();
});

it("reports an ended shell and ignores later input", async () => {
  native.write.mockRejectedValue({ code: "ended" });
  const onStatus = vi.fn();
  render(
    <TerminalDock
      scope={scope}
      session={handle}
      ended={false}
      onStatus={onStatus}
    />,
  );
  await waitFor(() => expect(xterm.data).not.toBeNull());
  act(() => xterm.data?.("x"));
  await waitFor(() =>
    expect(onStatus).toHaveBeenCalledWith("Session ended", true),
  );
  act(() => xterm.data?.("y"));
  expect(native.write).toHaveBeenCalledTimes(1);
});

it("sends input again while the program is not reading it", async () => {
  native.write
    .mockRejectedValueOnce({ code: "busy" })
    .mockRejectedValueOnce({ code: "busy" });
  const onStatus = vi.fn();
  render(
    <TerminalDock
      scope={scope}
      session={handle}
      ended={false}
      onStatus={onStatus}
    />,
  );
  await waitFor(() => expect(xterm.data).not.toBeNull());
  act(() => xterm.data?.("paste"));
  await waitFor(() => expect(native.write).toHaveBeenCalledTimes(3));
  expect(native.write.mock.calls.map((call) => call[2])).toEqual([
    "paste",
    "paste",
    "paste",
  ]);
  expect(onStatus).not.toHaveBeenCalled();
});

it("drops input refused as busy for too long and says why", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  try {
    native.write.mockRejectedValue({ code: "busy" });
    const onStatus = vi.fn();
    render(
      <TerminalDock
        scope={scope}
        session={handle}
        ended={false}
        onStatus={onStatus}
      />,
    );
    await waitFor(() => expect(xterm.data).not.toBeNull());
    act(() => xterm.data?.("x"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(11_000);
    });
    await waitFor(() =>
      expect(onStatus).toHaveBeenCalledWith(
        "Terminal operation failed.",
        false,
      ),
    );
    const attempts = native.write.mock.calls.length;
    expect(attempts).toBeGreaterThan(2);
    expect(attempts).toBeLessThan(40);
  } finally {
    vi.useRealTimers();
  }
});

it("retries a transient native reader conflict after remount", async () => {
  native.read.mockRejectedValueOnce({ code: "busy" });
  const onStatus = vi.fn();
  render(
    <TerminalDock
      scope={scope}
      session={handle}
      ended={false}
      onStatus={onStatus}
    />,
  );
  await waitFor(() => expect(native.read).toHaveBeenCalledTimes(2));
  expect(onStatus).not.toHaveBeenCalled();
});

it("resizes the active terminal after a pane or DPI change", async () => {
  render(
    <TerminalDock
      scope={scope}
      session={handle}
      ended={false}
      onStatus={() => undefined}
    />,
  );
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

it("closes every owned session when the focused chat changes", async () => {
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

it("uses the native transition epoch for a valid project scope", async () => {
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
    80,
    24,
  );
});

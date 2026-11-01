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
  options: null as Record<string, unknown> | null,
  measured: true,
  scrollbar: 0,
  cell: [8, 16],
  initial: [80, 24],
}));
let ptySize = { columns: 80, rows: 24 };
let resizeCallback: (() => void) | null = null;

vi.mock(import("../services/userTerminalDesktop"), async (importOriginal) => ({
  ...(await importOriginal()),
  createUserTerminal: native.create,
  readUserTerminal: native.read,
  closeUserTerminal: native.close,
  resizeUserTerminal: native.resize,
  writeUserTerminal: native.write,
  terminalErrorLabel: (error: unknown) =>
    (error as { code?: string }).code === "ended"
      ? "Session ended"
      : "Terminal operation failed.",
}));

vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    cols = xterm.initial[0];
    rows = xterm.initial[1];
    _core = {
      _renderService: {
        get dimensions() {
          return {
            device: {
              cell: {
                width: xterm.measured ? xterm.cell[0] * devicePixelRatio : 0,
                height: xterm.measured ? xterm.cell[1] * devicePixelRatio : 0,
              },
            },
          };
        },
      },
    };
    constructor(options: Record<string, unknown>) {
      xterm.options = options;
    }
    open(parent: HTMLElement) {
      // Like xterm, the screen is cols by rows cells of xterm.cell CSS px,
      // rounded to whole pixels, with an overlay scrollbar.
      const screen = document.createElement("div");
      screen.className = "xterm-screen";
      screen.getBoundingClientRect = () =>
        DOMRect.fromRect({
          width: xterm.measured ? Math.round(this.cols * xterm.cell[0]) : 0,
          height: xterm.measured ? Math.round(this.rows * xterm.cell[1]) : 0,
        });
      const scrollbar = document.createElement("div");
      scrollbar.className = "scrollbar vertical";
      scrollbar.getBoundingClientRect = () =>
        DOMRect.fromRect({ width: xterm.scrollbar, height: 0 });
      parent.append(screen, scrollbar);
    }
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

// The dock's screen box; it opens just large enough for xterm's 80 x 24.
const space = { width: 645, height: 390 };
for (const [property, axis] of [
  ["clientWidth", "width"],
  ["clientHeight", "height"],
] as const) {
  Object.defineProperty(HTMLElement.prototype, property, {
    configurable: true,
    get(this: HTMLElement) {
      return this.classList.contains("terminal-dock__screen") ? space[axis] : 0;
    },
  });
}

function setScreenSize(width: number, height: number) {
  space.width = width;
  space.height = height;
}

function expectGrid(
  container: HTMLElement,
  width: number,
  height: number,
  columns: number,
  rows: number,
) {
  const grid = container
    .querySelector(".xterm-screen")!
    .getBoundingClientRect();
  expect(grid.width).toBe(width);
  expect(grid.height).toBe(height);
  expect(ptySize).toEqual({ columns, rows });
}

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
  setScreenSize(645, 390);
  xterm.measured = true;
  xterm.scrollbar = 0;
  xterm.cell = [8, 16];
  xterm.initial = [80, 24];
  ptySize = { columns: 80, rows: 24 };
  Object.values(native).forEach((mock) => mock.mockReset());
  native.create.mockResolvedValue(handle);
  native.read.mockImplementation(() => new Promise(() => undefined));
  native.close.mockResolvedValue(undefined);
  native.resize.mockImplementation(async (_scope, _handle, columns, rows) => {
    ptySize = { columns, rows };
  });
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
  const { container } = render(
    <TerminalDock
      scope={scope}
      session={handle}
      ended={false}
      onStatus={() => undefined}
    />,
  );
  setScreenSize(804, 170);
  resizeCallback?.();
  await waitFor(() => expectGrid(container, 800, 160, 100, 10));
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

it.each([1, 80])(
  "fills an exact-fit container starting at %i columns",
  async (columns) => {
    xterm.initial = [columns, 24];
    setScreenSize(800, 160);
    const { container } = render(
      <TerminalDock
        scope={scope}
        session={handle}
        ended={false}
        onStatus={() => undefined}
      />,
    );
    await waitFor(() => expectGrid(container, 800, 160, 100, 10));
  },
);

it.each([
  [16, 16],
  [1600, 320],
])(
  "fits the same grid after resizing from %i by %i pixels",
  async (width, height) => {
    setScreenSize(width, height);
    const { container } = render(
      <TerminalDock
        scope={scope}
        session={handle}
        ended={false}
        onStatus={() => undefined}
      />,
    );
    setScreenSize(800, 160);
    resizeCallback?.();
    await waitFor(() => expectGrid(container, 800, 160, 100, 10));
    // Cross to the other side, then return to the identical destination.
    setScreenSize(width === 16 ? 1600 : 16, height === 16 ? 320 : 16);
    resizeCallback?.();
    await waitFor(() =>
      expectGrid(
        container,
        width === 16 ? 1600 : 16,
        height === 16 ? 320 : 16,
        width === 16 ? 200 : 2,
        height === 16 ? 20 : 1,
      ),
    );
    setScreenSize(800, 160);
    resizeCallback?.();
    await waitFor(() => expectGrid(container, 800, 160, 100, 10));
  },
);

it("keeps a measured grid inside the native size limits", async () => {
  const { container } = render(
    <TerminalDock
      scope={scope}
      session={handle}
      ended={false}
      onStatus={() => undefined}
    />,
  );
  setScreenSize(8 * 900, 16 * 400);
  resizeCallback?.();
  await waitFor(() => expectGrid(container, 4000, 4800, 500, 300));
  setScreenSize(1, 1);
  resizeCallback?.();
  await waitFor(() => expectGrid(container, 16, 16, 2, 1));
});

it("keeps xterm's overlay scrollbar clear of the last columns", async () => {
  const { container } = render(
    <TerminalDock
      scope={scope}
      session={handle}
      ended={false}
      onStatus={() => undefined}
    />,
  );
  xterm.scrollbar = 14;
  setScreenSize(814, 160);
  resizeCallback?.();
  await waitFor(() => expectGrid(container, 800, 160, 100, 10));
});

it.each([1, 80])(
  "fits fractional cells from %i columns without clipping",
  async (columns) => {
    // A 7.2 px cell: 80 columns render 576 px wide, 97 columns 698.4 px.
    xterm.cell = [7.2, 15.5];
    xterm.initial = [columns, 24];
    const { container } = render(
      <TerminalDock
        scope={scope}
        session={handle}
        ended={false}
        onStatus={() => undefined}
      />,
    );
    setScreenSize(700, 170);
    resizeCallback?.();
    await waitFor(() => expectGrid(container, 698, 155, 97, 10));
  },
);
it("keeps the grid until xterm has laid out its screen", async () => {
  xterm.measured = false;
  const { container } = render(
    <TerminalDock
      scope={scope}
      session={handle}
      ended={false}
      onStatus={() => undefined}
    />,
  );
  setScreenSize(800, 160);
  resizeCallback?.();
  await waitFor(() => expect(ptySize).toEqual({ columns: 80, rows: 24 }));
  xterm.measured = true;
  resizeCallback?.();
  await waitFor(() => expectGrid(container, 800, 160, 100, 10));
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

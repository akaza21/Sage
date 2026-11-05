import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { StrictMode, useState } from "react";

const native = vi.hoisted(() => ({
  create: vi.fn(),
  close: vi.fn(),
}));

vi.mock(import("../services/userTerminalDesktop"), async (importOriginal) => ({
  ...(await importOriginal()),
  createUserTerminal: native.create,
  closeUserTerminal: native.close,
  terminalErrorLabel: () => "Terminal operation failed.",
}));

vi.mock("./TerminalDock", () => ({
  default: ({
    session,
    ended,
    onStatus,
    visible = true,
  }: {
    session: { sessionId: string };
    ended: boolean;
    onStatus: (status: string, ended?: boolean) => void;
    visible?: boolean;
  }) =>
    visible ? (
      <div>
        Active session {session.sessionId}
        <span data-testid="terminal-ended-state">
          {ended ? "ended" : "live"}
        </span>
        <button type="button" onClick={() => onStatus("Session ended", false)}>
          Report status text only
        </button>
      </div>
    ) : null,
}));

import TerminalDockForWorkbench, {
  TerminalDockShell,
} from "./TerminalDockShell";
import { useWorkbenchStore } from "../app/store";

const scope = {
  projectId: "project-1",
  projectGeneration: "2",
  chatId: "chat-1",
};

const session = (index: number) => ({
  sessionId: `f99cd56a-32d9-4600-a1f6-${String(index).padStart(12, "0")}`,
  revision: "1",
  shellPath: "C:\\Windows\\System32\\cmd.exe",
});

beforeEach(() => {
  native.create.mockReset();
  native.close.mockReset();
  let index = 0;
  native.create.mockImplementation(async () => session(++index));
  native.close.mockResolvedValue(undefined);
});

afterEach(() => cleanup());

it("releases the terminal placeholder while its host is covered and restores it on reveal", () => {
  const original = useWorkbenchStore.getState();
  useWorkbenchStore.setState({ project: null, tabs: [] });
  const props = {
    container: document.body,
    pinnedChatId: null,
    setOpen: () => undefined,
    open: true,
  };
  const { rerender, unmount } = render(
    <TerminalDockForWorkbench {...props} visible />,
  );
  try {
    expect(screen.getByText("Open a chat first.")).toBeVisible();
    rerender(<TerminalDockForWorkbench {...props} visible={false} />);
    expect(screen.queryByText("Open a chat first.")).not.toBeInTheDocument();
    rerender(<TerminalDockForWorkbench {...props} visible />);
    expect(screen.getByText("Open a chat first.")).toBeVisible();
  } finally {
    unmount();
    useWorkbenchStore.setState(original, true);
  }
});

it("opens a usable replacement after the last terminal tab was explicitly ended", async () => {
  function Host() {
    const [visible, setVisible] = useState(true);
    return (
      <>
        <button onClick={() => setVisible(true)}>Show dock</button>
        <TerminalDockShell
          open
          scope={scope}
          visible={visible}
          onClose={() => setVisible(false)}
        />
      </>
    );
  }
  render(<Host />);
  await screen.findByText(`Active session ${session(1).sessionId}`);
  fireEvent.click(screen.getByRole("button", { name: "Close terminal tab" }));
  await waitFor(() =>
    expect(
      screen.queryByRole("region", { name: "Terminal" }),
    ).not.toBeInTheDocument(),
  );
  fireEvent.click(screen.getByRole("button", { name: "Show dock" }));
  expect(
    await screen.findByText(`Active session ${session(2).sessionId}`),
  ).toBeVisible();
});

it("hides and reopens the dock with its selected sessions intact", async () => {
  const onClose = () => undefined;
  const { rerender } = render(
    <TerminalDockShell open scope={scope} onClose={onClose} visible />,
  );
  await screen.findByText(`Active session ${session(1).sessionId}`);
  fireEvent.click(screen.getByRole("button", { name: "New terminal" }));
  await screen.findByText(`Active session ${session(2).sessionId}`);
  rerender(
    <TerminalDockShell open scope={scope} onClose={onClose} visible={false} />,
  );
  expect(
    screen.queryByRole("region", { name: "Terminal" }),
  ).not.toBeInTheDocument();
  rerender(<TerminalDockShell open scope={scope} onClose={onClose} visible />);
  expect(
    await screen.findByText(`Active session ${session(2).sessionId}`),
  ).toBeVisible();
  expect(screen.getAllByRole("tab")).toHaveLength(2);
  fireEvent.click(screen.getAllByRole("tab")[0]);
  expect(
    await screen.findByText(`Active session ${session(1).sessionId}`),
  ).toBeVisible();
});

it("creates a second tab, switches sessions, and closes only the selected tab", async () => {
  const onClose = vi.fn();
  render(<TerminalDockShell visible open scope={scope} onClose={onClose} />);
  expect(
    await screen.findByText(`Active session ${session(1).sessionId}`),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "New terminal" }));
  expect(
    await screen.findByText(`Active session ${session(2).sessionId}`),
  ).toBeInTheDocument();
  expect(screen.getAllByRole("tab")).toHaveLength(2);
  fireEvent.click(screen.getAllByRole("tab")[0]);
  expect(
    screen.getByText(`Active session ${session(1).sessionId}`),
  ).toBeInTheDocument();
  fireEvent.click(
    screen.getAllByRole("button", { name: "Close terminal tab" })[0],
  );
  await waitFor(() =>
    expect(native.close).toHaveBeenCalledWith(scope, session(1)),
  );
  expect(
    await screen.findByText(`Active session ${session(2).sessionId}`),
  ).toBeInTheDocument();
  expect(onClose).not.toHaveBeenCalled();
});

it("collapses without closing sessions and blocks a fifth tab locally", async () => {
  render(
    <TerminalDockShell visible open scope={scope} onClose={() => undefined} />,
  );
  await screen.findByRole("tab");
  for (let index = 0; index < 3; index++) {
    fireEvent.click(screen.getByRole("button", { name: "New terminal" }));
    await waitFor(() =>
      expect(screen.getAllByRole("tab")).toHaveLength(index + 2),
    );
  }
  expect(screen.getByRole("button", { name: "New terminal" })).toBeDisabled();
  expect(native.create).toHaveBeenCalledTimes(4);
  fireEvent.click(screen.getByRole("button", { name: "Collapse terminal" }));
  expect(
    screen.queryByText(`Active session ${session(4).sessionId}`),
  ).not.toBeInTheDocument();
  expect(native.close).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Expand terminal" }));
  expect(
    await screen.findByText(`Active session ${session(4).sessionId}`),
  ).toBeInTheDocument();
});

it("does not infer native ended state from status display text", async () => {
  render(
    <TerminalDockShell visible open scope={scope} onClose={() => undefined} />,
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "Report status text only" }),
  );
  expect(screen.getByTestId("terminal-ended-state")).toHaveTextContent("live");
  expect(screen.getByRole("status")).toHaveTextContent("Session ended");
});

it("selects a remaining tab when the closing tab was selected during native close", async () => {
  let finishClose: (() => void) | undefined;
  native.close.mockImplementationOnce(
    () =>
      new Promise<void>((resolve) => {
        finishClose = resolve;
      }),
  );
  render(
    <TerminalDockShell visible open scope={scope} onClose={() => undefined} />,
  );
  await screen.findByRole("tab");
  fireEvent.click(screen.getByRole("button", { name: "New terminal" }));
  await waitFor(() => expect(screen.getAllByRole("tab")).toHaveLength(2));
  fireEvent.click(
    screen.getAllByRole("button", { name: "Close terminal tab" })[0],
  );
  fireEvent.click(screen.getAllByRole("tab")[0]);
  finishClose?.();
  expect(
    await screen.findByText(`Active session ${session(2).sessionId}`),
  ).toBeInTheDocument();
  expect(screen.getAllByRole("tab")).toHaveLength(1);
});

it("dismisses after a pending replacement creation fails following last-tab close", async () => {
  let failCreate: ((error: Error) => void) | undefined;
  native.create.mockImplementationOnce(async () => session(1));
  native.create.mockImplementationOnce(
    () =>
      new Promise((_, reject) => {
        failCreate = reject;
      }),
  );
  const onClose = vi.fn();
  render(<TerminalDockShell visible open scope={scope} onClose={onClose} />);
  await screen.findByRole("tab");
  fireEvent.click(screen.getByRole("button", { name: "New terminal" }));
  await waitFor(() => expect(native.create).toHaveBeenCalledTimes(2));
  fireEvent.click(screen.getByRole("button", { name: "Close terminal tab" }));
  await waitFor(() => expect(screen.queryAllByRole("tab")).toHaveLength(0));
  expect(onClose).not.toHaveBeenCalled();
  failCreate?.(new Error("creation failed"));
  await waitFor(() => expect(onClose).toHaveBeenCalledOnce());
});

it("does not retain a stale creation across a StrictMode remount", async () => {
  const resolves: Array<(value: ReturnType<typeof session>) => void> = [];
  native.create.mockImplementation(
    () => new Promise((resolve) => resolves.push(resolve)),
  );
  render(
    <StrictMode>
      <TerminalDockShell visible open scope={scope} onClose={() => undefined} />
    </StrictMode>,
  );
  await waitFor(() => expect(native.create).toHaveBeenCalledTimes(1));
  resolves[0](session(1));
  expect(
    await screen.findByText(`Active session ${session(1).sessionId}`),
  ).toBeInTheDocument();
  expect(screen.getAllByRole("tab")).toHaveLength(1);
  expect(native.close).not.toHaveBeenCalled();
});

import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { StrictMode } from "react";

const native = vi.hoisted(() => ({
  create: vi.fn(),
  close: vi.fn(),
}));

vi.mock("../services/userTerminalDesktop", () => ({
  createUserTerminal: native.create,
  closeUserTerminal: native.close,
}));

vi.mock("./TerminalDock", () => ({
  TerminalDock: ({ session }: { session: { sessionId: string } }) => (
    <div>Active session {session.sessionId}</div>
  ),
}));

import { TerminalDockShell } from "./TerminalDockShell";

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

it("creates a second tab, switches sessions, and closes only the selected tab", async () => {
  const onClose = vi.fn();
  render(<TerminalDockShell scope={scope} onClose={onClose} />);
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
  render(<TerminalDockShell scope={scope} onClose={() => undefined} />);
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

it("does not retain a stale creation across a StrictMode remount", async () => {
  const resolves: Array<(value: ReturnType<typeof session>) => void> = [];
  native.create.mockImplementation(
    () => new Promise((resolve) => resolves.push(resolve)),
  );
  render(
    <StrictMode>
      <TerminalDockShell scope={scope} onClose={() => undefined} />
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

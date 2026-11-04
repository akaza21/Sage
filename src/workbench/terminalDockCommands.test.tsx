import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useTerminalDockStore } from "../app/terminalDockStore";
import { connectTerminalDock } from "./terminalDockCommands";
import { DISCONNECTED_TERMINAL } from "./terminalDockProjection";

vi.mock("../services/terminalDockSettingsDesktop", () => ({
  getTerminalDockVisible: async () => false,
  setTerminalDockVisible: async () => undefined,
}));

let disconnect: (() => void) | undefined;
afterEach(async () => {
  disconnect?.();
  disconnect = undefined;
  cleanup();
  await useTerminalDockStore.getState().flush();
});

async function openDock() {
  render(
    <>
      <input aria-label="Chat prompt" />
      <button className="status-bar__terminal">Terminal</button>
    </>,
  );
  let dock = DISCONNECTED_TERMINAL;
  await act(async () => {
    disconnect = connectTerminalDock(
      () => false,
      (next) => {
        dock = next;
      },
    );
  });
  const input = screen.getByRole("textbox");
  input.focus();
  act(() => dock.open());
  return { input, close: () => act(() => dock.setOpen(false)) };
}

it("returns focus to the connected prompt when the terminal closes", async () => {
  const { input, close } = await openDock();
  screen.getByRole("button").focus();
  close();
  expect(input).toHaveFocus();
});

it("forgets a disconnected prompt even if that element is later reattached", async () => {
  const { input, close } = await openDock();
  const parent = input.parentElement!;
  await act(async () => {
    input.remove();
  });
  parent.append(input);
  close();
  expect(screen.getByRole("button")).toHaveFocus();
});

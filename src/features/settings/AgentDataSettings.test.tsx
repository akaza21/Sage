import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import { resetWorkbenchPortsForTests } from "../../ports/workbenchRuntime";
import { AgentDataSettings } from "./AgentDataSettings";

const DAY = 86_400_000;
const invoke = vi.hoisted(() =>
  vi.fn(async (command: string, _args?: unknown): Promise<unknown> => {
    if (command === "count_stale_chats") return 3;
    throw new Error("refused");
  }),
);
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

// Native runtime, so the cleanup goes to the mocked Tauri command.
Object.assign(window, { __TAURI_INTERNALS__: {} });
resetWorkbenchPortsForTests();
const initialState = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  invoke.mockClear();
  useWorkbenchStore.setState(initialState, true);
});

it("confirms with the native count and deletes at the counted cutoff", async () => {
  useWorkbenchStore.setState({ nativeTransitionEpoch: 5 });
  const before = Date.now();
  render(<AgentDataSettings />);
  fireEvent.change(
    screen.getByRole("combobox", { name: "Delete chats older than" }),
    { target: { value: "30" } },
  );
  fireEvent.click(screen.getByRole("button", { name: "Delete…" }));

  const dialog = await screen.findByRole("dialog");
  expect(dialog).toHaveTextContent("Delete 3 chats?");
  expect(dialog).toHaveTextContent("not used in the last 30 days");
  const { cutoff } = invoke.mock.calls[0][1] as { cutoff: number };
  expect(cutoff).toBeGreaterThanOrEqual(before - 30 * DAY);
  expect(cutoff).toBeLessThanOrEqual(Date.now() - 30 * DAY);

  fireEvent.click(screen.getByRole("button", { name: "Delete 3 chats" }));
  await waitFor(() =>
    expect(invoke).toHaveBeenCalledWith("delete_stale_chats", {
      cutoff,
      expectedTransitionEpoch: 5,
    }),
  );
});

it("deletes nothing when no chat is old enough", async () => {
  invoke.mockImplementationOnce(async () => 0);
  render(<AgentDataSettings />);
  fireEvent.click(screen.getByRole("button", { name: "Delete…" }));

  expect(await screen.findByRole("status")).toHaveTextContent(
    "No chats older than 90 days.",
  );
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(invoke).toHaveBeenCalledTimes(1);
});

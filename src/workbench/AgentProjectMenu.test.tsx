import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../app/store";
import { AgentProjectMenu } from "./AgentProjectMenu";

const invoke = vi.hoisted(() =>
  vi.fn(async (_command: string, _args?: unknown) => {
    throw new Error("refused");
  }),
);
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

// Native runtime, so the cleanup goes to the mocked Tauri command.
Object.assign(window, { __TAURI_INTERNALS__: {} });
const initialState = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  invoke.mockClear();
  useWorkbenchStore.setState(initialState, true);
});

it("removes a project natively only after the permanent removal is confirmed", async () => {
  useWorkbenchStore.setState({ nativeTransitionEpoch: 7 });
  render(
    <AgentProjectMenu
      projectId="project-old"
      name="Old"
      onCreateChat={() => {}}
      onOpenProject={() => {}}
    />,
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Project actions for Old" }),
  );
  const remove = screen.getByRole("menuitem", { name: "Remove project…" });
  expect(remove).toBeEnabled();

  fireEvent.click(remove);
  expect(screen.getByRole("dialog")).toHaveTextContent("Remove Old?");
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(invoke).not.toHaveBeenCalled();

  fireEvent.click(
    screen.getByRole("button", { name: "Project actions for Old" }),
  );
  fireEvent.click(screen.getByRole("menuitem", { name: "Remove project…" }));
  fireEvent.click(screen.getByRole("button", { name: "Remove project" }));
  await waitFor(() =>
    expect(invoke).toHaveBeenCalledWith("remove_project", {
      projectId: "project-old",
      expectedTransitionEpoch: 7,
    }),
  );
  await waitFor(() =>
    expect(useWorkbenchStore.getState()).toMatchObject({
      projectTransitioning: false,
      error: "refused",
    }),
  );
});

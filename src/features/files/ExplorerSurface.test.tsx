import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import { ExplorerSurface } from "./ExplorerSurface";

const initialState = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
});

it("keeps Add folder root on Editor Explorer, not as a chat blocker", () => {
  const addProjectRoot = vi.fn(async () => undefined);
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "A", roots: [] },
    addProjectRoot,
  });

  render(<ExplorerSurface />);
  fireEvent.click(screen.getByRole("button", { name: "Add folder root" }));
  expect(addProjectRoot).toHaveBeenCalledTimes(1);
});

it("asks to open a folder when Explorer has no project roots", () => {
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "A", roots: [] },
  });

  render(<ExplorerSurface />);

  expect(screen.getByText("Open a folder to see files.")).toBeInTheDocument();
});

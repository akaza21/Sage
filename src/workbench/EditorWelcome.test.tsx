import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../app/store";
import { EditorWelcome } from "./EditorWelcome";

const initialState = useWorkbenchStore.getState();

afterEach(() => {
  useWorkbenchStore.setState(initialState, true);
});

describe("EditorWelcome", () => {
  it("offers only working Editor and Agent destinations", () => {
    const onOpenAgent = vi.fn();
    useWorkbenchStore.setState({ sidebarOpen: false, sidebarView: "outline" });
    render(<EditorWelcome onOpenAgent={onOpenAgent} />);

    fireEvent.click(screen.getByRole("button", { name: "Browse files" }));
    expect(useWorkbenchStore.getState()).toMatchObject({
      sidebarOpen: true,
      sidebarView: "explorer",
    });

    fireEvent.click(screen.getByRole("button", { name: "Search files" }));
    expect(useWorkbenchStore.getState().sidebarView).toBe("search");

    fireEvent.click(screen.getByRole("button", { name: "Open Agent" }));
    expect(onOpenAgent).toHaveBeenCalledOnce();
  });
});

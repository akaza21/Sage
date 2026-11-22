import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../../app/store";
import { ComposerBranchChip } from "./ComposerBranchChip";
import type * as LazyPartsModule from "./lazyParts";

const loader = vi.hoisted(() => ({
  load: async () => import("../../workbench/StatusBarDetails"),
}));
vi.mock("./lazyParts", async (original) => {
  const actual = await original<typeof LazyPartsModule>();
  return {
    ...actual,
    lazyParts: () => actual.lazyParts(() => loader.load()),
  };
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  useWorkbenchStore.setState(useWorkbenchStore.getInitialState(), true);
});

it("retries a rejected branch menu import from the trigger and opens the recovered menu", async () => {
  vi.spyOn(console, "error").mockImplementation(() => undefined);
  let first = true;
  vi.spyOn(loader, "load").mockImplementation(async () => {
    if (first) {
      first = false;
      throw new Error("Chunk unavailable");
    }
    return import("../../workbench/StatusBarDetails");
  });
  useWorkbenchStore.setState({
    project: {
      id: "project-a",
      name: "Alpha",
      roots: [
        {
          id: "root-a",
          name: "Alpha",
          ordinal: 0,
          availability: "available",
          message: null,
        },
      ],
    },
    repositories: {
      roots: [
        {
          rootId: "root-a",
          state: "repository",
          branch: "main",
          branches: ["main", "topic"],
          changes: [],
          truncated: false,
          elapsedMs: 1,
          message: null,
        },
      ],
      truncated: false,
      elapsedMs: 1,
    },
  });
  render(<ComposerBranchChip disabled={false} />);
  fireEvent.click(
    screen.getByRole("button", { name: "Switch branch in Alpha" }),
  );
  const retry = await screen.findByRole("button", {
    name: "Retry loading branches in Alpha",
  });
  expect(retry).toBeEnabled();
  expect(retry).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByRole("listbox")).toBeNull();
  fireEvent.click(retry);
  const menu = await screen.findByRole("listbox", {
    name: "Switch branch in Alpha",
  });
  expect(
    within(menu).getByRole("option", { name: "Switch to topic" }),
  ).toBeEnabled();
  fireEvent.keyDown(screen.getByPlaceholderText("Select a branch"), {
    key: "Escape",
  });
  await waitFor(() => expect(screen.queryByRole("listbox")).toBeNull());
});

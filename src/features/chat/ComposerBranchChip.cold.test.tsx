import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
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
  return { ...actual, lazyParts: () => actual.lazyParts(() => loader.load()) };
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  useWorkbenchStore.setState(useWorkbenchStore.getInitialState(), true);
});

function seedRepository(store = useWorkbenchStore) {
  store.setState({
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
}

it("dismisses the pending branch menu and keeps moved focus while its module loads", async () => {
  let resolve!: () => void;
  const gate = new Promise<void>((done) => (resolve = done));
  vi.spyOn(loader, "load").mockImplementation(async () => {
    await gate;
    return import("../../workbench/StatusBarDetails");
  });
  seedRepository();
  render(
    <>
      <ComposerBranchChip disabled={false} />
      <button type="button">Elsewhere</button>
    </>,
  );
  const chip = screen.getByRole("button", { name: "Switch branch in Alpha" });
  chip.focus();
  fireEvent.click(chip);
  expect(chip).toHaveAttribute("aria-expanded", "true");
  fireEvent.keyDown(chip, { key: "Escape" });
  expect(chip).toHaveAttribute("aria-expanded", "false");
  expect(chip).toHaveFocus();

  fireEvent.keyDown(chip, { key: "ArrowDown" });
  expect(chip).toHaveAttribute("aria-expanded", "true");
  fireEvent.pointerDown(document.body);
  expect(chip).toHaveAttribute("aria-expanded", "false");

  fireEvent.click(chip);
  const elsewhere = screen.getByRole("button", { name: "Elsewhere" });
  elsewhere.focus();
  await act(async () => resolve());
  const loaded = await screen.findByRole("button", {
    name: "Switch branch in Alpha",
    expanded: false,
  });
  expect(screen.queryByRole("listbox")).toBeNull();
  expect(elsewhere).toHaveFocus();
  fireEvent.click(loaded);
  expect(
    await screen.findByRole("listbox", { name: "Switch branch in Alpha" }),
  ).toBeVisible();
});

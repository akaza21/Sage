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
import { desktop } from "../../services/desktop";
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
  seedRepository();
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

it("shows the native refusal while another chat works in the repository, then switches once its turn ends", async () => {
  seedRepository();
  // Chat A's turn, as the native repository guard sees it. Chat B (this
  // composer) is idle, so its own chip stays enabled.
  let chatATurn: "running" | "completed" = "running";
  vi.spyOn(desktop, "checkoutGitLocalBranch").mockImplementation(
    async (request, epoch) => {
      if (chatATurn === "running")
        // GitFileMutationCommandError::Failed as the checkout command
        // serializes it.
        throw {
          kind: "failed",
          message:
            "Can't switch branches while an agent is working in this repository.",
        };
      const state = useWorkbenchStore.getState();
      return {
        projectId: request.projectId,
        rootId: request.rootId,
        transitionEpoch: epoch!,
        error: null,
        repositories: {
          ...state.repositories,
          roots: [{ ...state.repositories.roots[0], branch: request.branch }],
        },
      };
    },
  );
  const chats = () => (
    <>
      <section aria-label="Chat A">
        <ComposerBranchChip
          disabled={false}
          activeTurn={chatATurn === "running"}
        />
      </section>
      <section aria-label="Chat B">
        <ComposerBranchChip disabled={false} activeTurn={false} />
      </section>
    </>
  );
  const { rerender } = render(chats());
  const chip = () =>
    within(screen.getByRole("region", { name: "Chat B" })).getByRole("button", {
      name: "Switch branch in Alpha",
    });
  const chatAChip = () =>
    within(screen.getByRole("region", { name: "Chat A" })).getByRole("button", {
      name: "Switch branch in Alpha",
    });
  expect(chatAChip()).toBeDisabled();
  expect(chatAChip()).toHaveAttribute(
    "title",
    "Can't switch branches while the agent is working",
  );
  const switchToTopic = async () => {
    fireEvent.click(chip());
    fireEvent.click(
      await screen.findByRole("option", { name: "Switch to topic" }),
    );
  };
  expect(chip()).toBeEnabled();
  await switchToTopic();
  const refusal = await screen.findByRole("dialog", {
    name: "Cannot switch branch",
  });
  expect(within(refusal).getByRole("alert")).toHaveTextContent(
    "Can't switch branches while an agent is working in this repository.",
  );
  expect(chip()).toHaveTextContent("main");
  expect(useWorkbenchStore.getState().repositoryCheckout).toBeNull();
  fireEvent.click(within(refusal).getByRole("button", { name: "Close" }));

  chatATurn = "completed";
  rerender(chats());
  expect(chatAChip()).toBeEnabled();
  await switchToTopic();
  await waitFor(() => expect(chip()).toHaveTextContent("topic"));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(chatAChip()).toHaveTextContent("topic");
  expect(useWorkbenchStore.getState().repositories.roots[0].branch).toBe(
    "topic",
  );
});

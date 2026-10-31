import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { useWorkbenchStore } from "../app/store";
import { useAgentSettingsState } from "../features/settings/agentSettingsState";
import { TitleBar } from "./TitleBar";
import { useSideChatState } from "./sideChatState";

const initial = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  useAgentSettingsState.getState().close();
  useWorkbenchStore.setState(initial, true);
});

function seed(count = 2) {
  useWorkbenchStore.setState({
    project: { id: "a", name: "A", roots: [] },
    projectTransitioning: false,
    tabs: Array.from({ length: count }, (_, index) => ({
      id: `chat-${index}`,
      kind: "chat" as const,
      title: `Chat ${index}`,
      position: index,
      dirty: false,
      pinned: false,
    })),
    activeTabId: "chat-0",
    activateTab: async (activeTabId) => {
      useWorkbenchStore.setState({ activeTabId });
    },
    createChat: async () => {
      throw new Error("History must never create a chat");
    },
  });
  render(<TitleBar mode="agent" onModeChange={() => {}} />);
}

it("travels chat A → B → Settings without creating chats or restoring a split", async () => {
  seed();
  expect(screen.getByRole("button", { name: "Go back" })).toBeDisabled();
  await act(async () => {
    await useWorkbenchStore.getState().activateTab("chat-1");
  });
  act(() => useAgentSettingsState.getState().open("Chats"));
  useSideChatState.setState({ pairedId: "side-test" });
  fireEvent.click(screen.getByRole("button", { name: "Go back" }));
  await waitFor(() => expect(useAgentSettingsState.getState().page).toBeNull());
  expect(useWorkbenchStore.getState().activeTabId).toBe("chat-1");
  expect(useSideChatState.getState().pairedId).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Go back" }));
  await waitFor(() =>
    expect(useWorkbenchStore.getState().activeTabId).toBe("chat-0"),
  );
  fireEvent.click(screen.getByRole("button", { name: "Go forward" }));
  await waitFor(() =>
    expect(useWorkbenchStore.getState().activeTabId).toBe("chat-1"),
  );
  fireEvent.click(screen.getByRole("button", { name: "Go forward" }));
  await waitFor(() =>
    expect(useAgentSettingsState.getState().page).toBe("Chats"),
  );
  expect(screen.getByRole("button", { name: "Go forward" })).toBeDisabled();
  expect(useWorkbenchStore.getState().tabs).toHaveLength(2);
});

it("evicts old entries and discards removed chats and project transitions", async () => {
  seed(40);
  for (let index = 1; index < 40; index++)
    act(() => useWorkbenchStore.setState({ activeTabId: `chat-${index}` }));
  for (let index = 0; index < 31; index++) {
    fireEvent.click(screen.getByRole("button", { name: "Go back" }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Go forward" })).toBeEnabled(),
    );
  }
  expect(useWorkbenchStore.getState().activeTabId).toBe("chat-8");
  expect(screen.getByRole("button", { name: "Go back" })).toBeDisabled();
  act(() =>
    useWorkbenchStore.setState((state) => ({
      tabs: state.tabs.filter((tab) => tab.id === "chat-8"),
    })),
  );
  expect(screen.getByRole("button", { name: "Go forward" })).toBeDisabled();
  act(() => useWorkbenchStore.setState({ projectTransitioning: true }));
  expect(screen.getByRole("button", { name: "Go back" })).toBeDisabled();
});

it("keeps its position when native activation refuses to change the destination", async () => {
  seed();
  act(() =>
    useWorkbenchStore.setState({
      activeTabId: "chat-1",
      activateTab: async () => {},
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Go back" }));
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "Go back" })).toBeEnabled(),
  );
  expect(useWorkbenchStore.getState().activeTabId).toBe("chat-1");
  expect(screen.getByRole("button", { name: "Go forward" })).toBeDisabled();
});

it("discards old-project history when a transition completes during activation", async () => {
  seed();
  let complete!: () => void;
  act(() =>
    useWorkbenchStore.setState({
      activeTabId: "chat-1",
      activateTab: () =>
        new Promise<void>((resolve) => {
          complete = resolve;
        }),
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Go back" }));
  await act(async () => {
    useWorkbenchStore.setState((state) => ({
      project: { id: "b", name: "B", roots: [] },
      projectGeneration: state.projectGeneration + 1,
      activeTabId: "chat-0",
    }));
    complete();
  });
  expect(screen.getByRole("button", { name: "Go back" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Go forward" })).toBeDisabled();
});

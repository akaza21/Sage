import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type * as NativeBrowserPort from "../ports/nativeBrowserPort";

vi.mock("../ports/nativeBrowserPort", async (importOriginal) => ({
  ...(await importOriginal<typeof NativeBrowserPort>()),
  closeBrowser: vi.fn().mockResolvedValue(undefined),
}));

import {
  recordBrowserAgentOperation,
  clearBrowserAgentPresence,
} from "../app/browserAgentActivityStore";
import { useWorkbenchStore } from "../app/store";
import { browserOperationFixture } from "../domain/browserOperation.fixture";
import { closeBrowser } from "../ports/nativeBrowserPort";
import { TabStrip } from "./TabStrip";
import { ActiveEditorCanvas } from "./ActiveEditorCanvas";
import { EDITOR_CANVAS_KINDS } from "./workbenchWindowMode";

afterEach(() => {
  cleanup();
  clearBrowserAgentPresence("browser-7");
});

it("marks a browser tab when an agent is using it and names the chat", () => {
  const previous = useWorkbenchStore.getState();
  useWorkbenchStore.setState({
    project: { id: "project-1" } as typeof previous.project,
    tabs: [
      {
        id: "browser-7",
        kind: "browser",
        title: "Browser",
        position: 0,
        pinned: false,
        dirty: false,
      },
      {
        id: "chat-1",
        kind: "chat",
        title: "Research chat",
        position: 1,
        pinned: false,
        dirty: false,
      },
    ],
  });
  recordBrowserAgentOperation({
    ...browserOperationFixture,
    phase: "started",
    updatedAt: Date.now(),
  });
  render(<TabStrip />);
  expect(
    screen.getByText("Agent has a browser action pending · Research chat"),
  ).toBeInTheDocument();
  useWorkbenchStore.setState({
    project: previous.project,
    tabs: previous.tabs,
  });
});

it("does not carry agent presence into a new browser with a reused tab ID", async () => {
  const previous = useWorkbenchStore.getState();
  const browser = {
    id: "browser-7",
    kind: "browser" as const,
    title: "Browser",
    position: 0,
    pinned: false,
    dirty: false,
  };
  const oldChat = {
    ...browser,
    id: "old-chat",
    kind: "chat" as const,
    title: "Old chat",
  };
  const newChat = {
    ...oldChat,
    id: "new-chat",
    title: "New chat",
  };
  const now = Date.now();
  useWorkbenchStore.setState({
    project: { id: "project-1" } as typeof previous.project,
    tabs: [browser, oldChat, newChat],
  });
  recordBrowserAgentOperation({
    ...browserOperationFixture,
    chatId: oldChat.id,
    operationId: "old-operation",
    phase: "started",
    updatedAt: now + 1_000,
  });
  render(<TabStrip />);
  expect(
    screen.getByText("Agent has a browser action pending · Old chat"),
  ).toBeInTheDocument();
  act(() => useWorkbenchStore.setState({ tabs: [oldChat, newChat] }));
  await vi.waitFor(() => expect(closeBrowser).toHaveBeenCalledWith(browser.id));
  await Promise.resolve();
  act(() => useWorkbenchStore.setState({ tabs: [browser, oldChat, newChat] }));
  expect(
    screen.queryByText("Agent has a browser action pending · Old chat"),
  ).toBeNull();
  act(() =>
    recordBrowserAgentOperation({
      ...browserOperationFixture,
      chatId: newChat.id,
      operationId: "new-operation",
      phase: "started",
      updatedAt: now,
    }),
  );
  expect(
    screen.getByText("Agent has a browser action pending · New chat"),
  ).toBeInTheDocument();
  useWorkbenchStore.setState({
    project: previous.project,
    tabs: previous.tabs,
  });
});

it("shows a browser favicon and falls back to the globe for an unillustrated tab", () => {
  const previous = useWorkbenchStore.getState();
  useWorkbenchStore.setState({
    tabs: [
      {
        id: "browser-icon",
        kind: "browser",
        title: "Example",
        position: 0,
        pinned: false,
        dirty: false,
        favicon: "data:image/png;base64,iVBORw0KGgo=",
      },
      {
        id: "browser-plain",
        kind: "browser",
        title: "Plain",
        position: 1,
        pinned: false,
        dirty: false,
      },
    ],
  });
  render(<TabStrip />);
  expect(
    screen.getByRole("tab", { name: "Example" }).querySelector("img"),
  ).toHaveAttribute("src", "data:image/png;base64,iVBORw0KGgo=");
  fireEvent.error(
    screen.getByRole("tab", { name: "Example" }).querySelector("img")!,
  );
  expect(
    screen.getByRole("tab", { name: "Example" }).querySelector("svg.lucide"),
  ).toBeInTheDocument();
  act(() =>
    useWorkbenchStore.setState((state) => ({
      tabs: state.tabs.map((item) =>
        item.id === "browser-icon"
          ? { ...item, favicon: "data:image/png;base64,iVBORw0KGgoAAA=" }
          : item,
      ),
    })),
  );
  expect(
    screen.getByRole("tab", { name: "Example" }).querySelector("img"),
  ).toHaveAttribute("src", "data:image/png;base64,iVBORw0KGgoAAA=");
  expect(
    screen.getByRole("tab", { name: "Plain" }).querySelector("svg.lucide"),
  ).toBeInTheDocument();
  useWorkbenchStore.setState({ tabs: previous.tabs });
});

it("keeps browser tabs and creation out of the Editor strip", () => {
  const previous = useWorkbenchStore.getState();
  useWorkbenchStore.setState({
    project: { id: "project-1" } as typeof previous.project,
    activeTabId: "browser-7",
    tabs: [
      {
        id: "browser-7",
        kind: "browser",
        title: "Browser",
        position: 0,
        pinned: false,
        dirty: false,
      },
      {
        id: "file-1",
        kind: "file",
        title: "file.ts",
        position: 1,
        pinned: false,
        dirty: false,
      },
    ],
  });
  render(
    <>
      <TabStrip kinds={[...EDITOR_CANVAS_KINDS]} showNewChat={false} />
      <ActiveEditorCanvas onOpenAgent={() => undefined} />
    </>,
  );
  expect(screen.queryByRole("tab", { name: "Browser" })).toBeNull();
  expect(screen.getByRole("tab", { name: "file.ts" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "New browser tab" })).toBeNull();
  expect(screen.queryByLabelText("Sage browser")).toBeNull();
  expect(
    screen.getByRole("region", { name: "Editor welcome" }),
  ).toBeInTheDocument();
  useWorkbenchStore.setState({
    project: previous.project,
    tabs: previous.tabs,
    activeTabId: previous.activeTabId,
  });
});

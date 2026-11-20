import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { browserDesktop } from "../../services/browserDesktop";
import { useChatEffortDraft } from "../../app/chatEffortDraft";
import {
  draftChatAccess,
  useChatAccessDrafts,
} from "../../app/chatAccessDrafts";
import { useWorkbenchStore } from "../../app/store";
import { resetBrowserDesktopForTests } from "../../services/browserDesktop";
import { ProjectScopeChip } from "./ProjectScopeChip";

const initial = useWorkbenchStore.getState();
beforeEach(async () => {
  resetBrowserDesktopForTests();
  useWorkbenchStore.setState(initial, true);
  await useWorkbenchStore.getState().initialize();
  await useWorkbenchStore.getState().openNewChatDraft();
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  useChatEffortDraft.setState({ draft: null });
  useChatAccessDrafts.setState({ drafts: {} });
  useWorkbenchStore.setState(initial, true);
});

it("keeps the entire unsent composer through New project creation and reparenting", async () => {
  vi.spyOn(getWorkbenchPorts().project, "createProject").mockImplementation(
    (_request, _expected, epoch) => browserDesktop.ensureLocalProject(epoch),
  );
  await act(async () => useWorkbenchStore.getState().createChat(null));
  const state = useWorkbenchStore.getState();
  const chat = state.activeTabId!;
  const owner = state.captureChatOwnership(chat)!;
  const context = {
    kind: "browserPage" as const,
    tabId: "browser-1",
    url: "https://example.test/",
    title: "Reference",
    text: "Keep this context",
  };
  state.setChatComposer(owner, "Keep this prompt");
  expect(state.attachChatContext(owner, context)).toBe(true);
  state.setChatHarnessSelection(owner, {
    harnessId: "sage-simulator",
    modelId: "sim-fast",
  });
  useChatEffortDraft.setState({
    draft: {
      chatId: chat,
      bindingId: "sage-simulator",
      modelId: "sim-fast",
      effort: "high",
    },
  });
  draftChatAccess(chat, { bindingId: "sage-simulator", mode: "fullAccess" });
  render(<ProjectScopeChip projectName="Choose project" />);
  fireEvent.click(screen.getByRole("button", { name: "Choose project" }));
  fireEvent.click(await screen.findByRole("option", { name: "New project" }));
  fireEvent.change(
    await screen.findByRole("textbox", { name: "Project name" }),
    { target: { value: "Local" } },
  );
  fireEvent.click(screen.getByRole("button", { name: "Create project" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(useWorkbenchStore.getState()).toMatchObject({
    activeTabId: chat,
    composerText: "Keep this prompt",
    project: { name: "Local" },
  });
  expect(useWorkbenchStore.getState().chatUiById[chat]).toMatchObject({
    composerText: "Keep this prompt",
    attachedContext: [context],
    harnessId: "sage-simulator",
    modelId: "sim-fast",
  });
  expect(useChatEffortDraft.getState().draft).toEqual({
    chatId: chat,
    bindingId: "sage-simulator",
    modelId: "sim-fast",
    effort: "high",
  });
  expect(useChatAccessDrafts.getState().drafts[chat]).toEqual({
    bindingId: "sage-simulator",
    mode: "fullAccess",
  });
});

it("starts without a project and lets the menu choose and clear the draft scope", async () => {
  useWorkbenchStore.setState({
    projects: [
      {
        id: "one",
        name: "Sage",
        parentPath: "/work/first",
        rootCount: 1,
        lastOpenedAt: 0,
      },
      {
        id: "two",
        name: "Sage",
        parentPath: "/work/second",
        rootCount: 1,
        lastOpenedAt: 0,
      },
    ],
  });
  render(<ProjectScopeChip projectName="Choose project" />);
  const chip = screen.getByRole("button", { name: "Choose project" });
  expect(chip).toHaveAttribute("title", "Change the project for this chat");
  fireEvent.click(chip);
  fireEvent.click(
    await screen.findByRole("option", { name: "Sage, /work/first" }),
  );
  expect(useWorkbenchStore.getState().newChatDraft?.targetProjectId).toBe(
    "one",
  );
  fireEvent.click(screen.getByRole("button", { name: "Project: Sage" }));
  expect(
    await screen.findByRole("option", { name: "New project" }),
  ).toBeVisible();
  fireEvent.click(
    screen.getByRole("option", { name: "Don't work in a project" }),
  );
  expect(screen.getByRole("button", { name: "Choose project" })).toBeVisible();
  expect(useWorkbenchStore.getState().newChatDraft?.targetProjectId).toBeNull();
});

it("offers a keyboard-accessible clear button and freezes the chip when disabled", async () => {
  await act(async () =>
    useWorkbenchStore
      .getState()
      .openNewChatDraft(useWorkbenchStore.getState().project!.id),
  );
  const { rerender } = render(<ProjectScopeChip projectName="Sage" />);
  fireEvent.click(
    screen.getByRole("button", { name: "Remove project from this chat" }),
  );
  expect(useWorkbenchStore.getState().newChatDraft?.targetProjectId).toBeNull();
  rerender(<ProjectScopeChip projectName="Choose project" disabled />);
  expect(screen.getByRole("button", { name: "Choose project" })).toBeDisabled();
  expect(
    screen.queryByRole("button", { name: "Remove project from this chat" }),
  ).toBeNull();
});

it("keeps an established session's scope read-only", () => {
  useWorkbenchStore.setState({
    agentCenter: "chat",
    newChatDraft: null,
    sessionId: "started-session",
  });
  render(<ProjectScopeChip projectName="Sage" />);
  const chip = screen.getByRole("button", { name: "Project: Sage" });
  expect(chip).toBeDisabled();
  fireEvent.click(chip);
  expect(screen.queryByRole("listbox")).toBeNull();
  expect(
    screen.queryByRole("button", { name: "Remove project from this chat" }),
  ).toBeNull();
});

import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { clearAcpProjectionRegistry } from "../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../app/store";
import { useAgentSettingsState } from "../features/settings/agentSettingsState";
import { resetBrowserDesktopForTests } from "../services/browserDesktop";
import { settleLazyModules } from "../test/settleLazyModules";
import { Workbench } from "./Workbench";
import { useSideChatState } from "./sideChatState";

// Isolate this mounted race so the draft actions chunk is not already cached.
const actionsLoad = vi.hoisted(() => {
  let release!: () => void;
  const ready = new Promise<void>((resolve) => {
    release = resolve;
  });
  return { ready, release };
});
vi.mock("../app/newChatDraftActions", async (importOriginal) => {
  await actionsLoad.ready;
  return importOriginal();
});

const initial = useWorkbenchStore.getState();

beforeEach(async () => {
  localStorage.clear();
  resetBrowserDesktopForTests();
  useWorkbenchStore.setState(initial, true);
  vi.stubGlobal("innerWidth", 1800);
  vi.stubGlobal(
    "matchMedia",
    vi.fn(() => ({
      matches: false,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    })),
  );
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  await useWorkbenchStore.getState().initialize();
});

afterEach(() => {
  cleanup();
  clearAcpProjectionRegistry();
  useSideChatState.setState({ sides: [], pairedId: null });
  useAgentSettingsState.getState().close();
  vi.unstubAllGlobals();
  useWorkbenchStore.setState(initial, true);
  resetBrowserDesktopForTests();
  localStorage.clear();
});

it("sends the first simulator prompt after switching to an empty project while New chat actions load", async () => {
  const state = useWorkbenchStore.getState();
  const originalProjectId = state.project!.id;
  expect(
    await state.createProject({ name: "Empty project", sourceTokens: [] }),
  ).toBe(true);
  const emptyProjectId = useWorkbenchStore.getState().project!.id;
  await state.switchProject(originalProjectId);
  render(<Workbench />);
  await settleLazyModules();

  try {
    fireEvent.click(screen.getByRole("button", { name: "New chat" }));
    expect(useWorkbenchStore.getState().newChatDraftOpening).toBe(true);
    await act(async () => state.switchProject(emptyProjectId));
    expect(useWorkbenchStore.getState().project?.id).toBe(emptyProjectId);
    expect(useWorkbenchStore.getState().tabs).toEqual([]);
    expect(useWorkbenchStore.getState().newChatDraft).toBeNull();
    expect(useWorkbenchStore.getState().newChatDraftOpening).toBe(true);
  } finally {
    actionsLoad.release();
    await settleLazyModules();
  }

  const draft = screen.getByRole("region", { name: "New chat draft" });
  const composer = within(draft).getByRole("combobox", {
    name: "Message agent",
  });
  expect(composer).toBeEnabled();
  fireEvent.change(composer, { target: { value: "Work in this project" } });
  expect(composer).toHaveValue("Work in this project");
  expect(useWorkbenchStore.getState().newChatDraft).toMatchObject({
    projectId: emptyProjectId,
    targetProjectId: emptyProjectId,
    text: "Work in this project",
  });
  const catalog = useWorkbenchStore.getState();
  expect(
    catalog.harnesses.filter((harness) => harness.installState === "available"),
  ).toMatchObject([{ transport: "legacySimulator", bindingId: null }]);
  const send = within(draft).getByRole("button", { name: "Send message" });
  expect(send).toBeEnabled();
  fireEvent.click(send);
  await waitFor(() =>
    expect(useWorkbenchStore.getState().messages).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ role: "user", text: "Work in this project" }),
        expect.objectContaining({ role: "assistant" }),
      ]),
    ),
  );
  const sent = useWorkbenchStore.getState();
  expect(sent.project?.id).toBe(emptyProjectId);
  expect(sent.newChatDraft).toBeNull();
  expect(sent.pendingFirstSend).toBeNull();
  expect(sent.sessionId).toBeTruthy();
  expect(sent.composerText).toBe("");
  expect(sent.tabs.filter((tab) => tab.kind === "chat")).toHaveLength(1);
  expect(screen.getByRole("combobox", { name: "Message agent" })).toHaveValue(
    "",
  );
  expect(screen.getByText("Work in this project")).toBeVisible();
});

it.each([
  { entry: "sidebar", text: "" },
  { entry: "sidebar", text: "Keep my unsent prompt" },
  { entry: "shortcut", text: "" },
  { entry: "shortcut", text: "Keep my unsent prompt" },
  { entry: "collapsed sidebar", text: "Keep my unsent prompt" },
  { entry: "palette", text: "Keep my unsent prompt" },
])(
  "global New chat from $entry clears the startup project and preserves '$text'",
  async ({ entry, text }) => {
    actionsLoad.release();
    const state = useWorkbenchStore.getState();
    await state.createProject({ name: "Empty project", sourceTokens: [] });
    const projectId = useWorkbenchStore.getState().project!.id;
    render(<Workbench />);
    await settleLazyModules();
    expect(useWorkbenchStore.getState().newChatDraft?.targetProjectId).toBe(
      projectId,
    );
    const draft = screen.getByRole("region", { name: "New chat draft" });
    const composer = within(draft).getByRole("combobox", {
      name: "Message agent",
    });
    fireEvent.change(composer, { target: { value: text } });
    if (entry === "collapsed sidebar") {
      fireEvent.click(
        screen.getByRole("button", { name: "Hide projects and chats" }),
      );
      await settleLazyModules();
    }
    if (entry === "sidebar" || entry === "collapsed sidebar")
      fireEvent.click(screen.getByRole("button", { name: "New chat" }));
    else if (entry === "shortcut")
      fireEvent.keyDown(window, { key: "n", ctrlKey: true });
    else {
      fireEvent.click(
        within(
          screen.getByRole("complementary", { name: "Projects and chats" }),
        ).getByRole("button", { name: "Search chats" }),
      );
      fireEvent.click(await screen.findByRole("option", { name: "New chat" }));
    }
    await settleLazyModules();
    expect(useWorkbenchStore.getState().newChatDraft).toMatchObject({
      targetProjectId: null,
      text,
    });
    expect(composer).toHaveValue(text);
    expect(
      within(draft).getByRole("button", { name: "Choose project" }),
    ).toBeVisible();
    expect(useWorkbenchStore.getState().tabs).toEqual([]);
  },
);

it("returns the project-less draft to its project from the project row's New chat", async () => {
  actionsLoad.release();
  const state = useWorkbenchStore.getState();
  await state.createProject({ name: "Empty project", sourceTokens: [] });
  const projectId = useWorkbenchStore.getState().project!.id;
  render(<Workbench />);
  await settleLazyModules();
  const draft = screen.getByRole("region", { name: "New chat draft" });
  const composer = within(draft).getByRole("combobox", {
    name: "Message agent",
  });
  fireEvent.change(composer, { target: { value: "Keep my unsent prompt" } });
  fireEvent.click(screen.getByRole("button", { name: "New chat" }));
  await settleLazyModules();
  expect(useWorkbenchStore.getState().newChatDraft?.targetProjectId).toBe(null);
  fireEvent.click(
    screen.getByRole("button", { name: "New chat in Empty project" }),
  );
  await settleLazyModules();
  expect(useWorkbenchStore.getState().newChatDraft).toMatchObject({
    targetProjectId: projectId,
    text: "Keep my unsent prompt",
  });
  expect(composer).toHaveValue("Keep my unsent prompt");
  expect(
    within(draft).queryByRole("button", { name: "Choose project" }),
  ).toBeNull();
  for (const chip of within(draft).getAllByRole("button", {
    name: "Project: Empty project",
  }))
    expect(chip).toBeVisible();
});

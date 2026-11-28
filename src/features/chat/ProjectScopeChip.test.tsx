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
import { browserDirectories } from "../../../tests/ui/simulator/browserWorkspaceFixture";
import { resourceKey } from "../../domain/resourceIdentity";
import { useChatEffortDraft } from "../../app/chatEffortDraft";
import {
  draftChatAccess,
  useChatAccessDrafts,
} from "../../app/chatAccessDrafts";
import { useWorkbenchStore } from "../../app/store";
import { resetBrowserDesktopForTests } from "../../../tests/ui/simulator/browserDesktop";
import { ProjectScopeChip } from "./ProjectScopeChip";
import { AgentCenterKeeper } from "../../workbench/WorkbenchContinuity";

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

it("searches projects and offers clearing only for a scoped chat", async () => {
  const state = useWorkbenchStore.getState();
  const project = state.projects[0]!;
  const view = render(<ProjectScopeChip projectName="Choose project" />);
  fireEvent.click(screen.getByRole("button", { name: "Choose project" }));
  expect(await screen.findByPlaceholderText("Search projects")).toBeVisible();
  expect(
    screen.queryByRole("option", { name: "Don't work in a project" }),
  ).toBeNull();
  const row = screen.getByRole("option", { name: project.name });
  expect(row.querySelector("svg")).toBeInTheDocument();
  fireEvent.click(row);
  view.rerender(<ProjectScopeChip projectName={project.name} />);
  fireEvent.click(
    screen.getByRole("button", { name: `Project: ${project.name}` }),
  );
  const selected = await screen.findByRole("option", { name: project.name });
  expect(selected).toHaveAttribute("aria-selected", "true");
  expect(
    selected.querySelector(".searchable-picker__selected-check"),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("option", { name: "Don't work in a project" }),
  ).toBeVisible();
  fireEvent.change(screen.getByPlaceholderText("Search projects"), {
    target: { value: "missing" },
  });
  await waitFor(() =>
    expect(screen.queryByRole("option", { name: project.name })).toBeNull(),
  );
  expect(screen.getByRole("option", { name: "New project" })).toBeVisible();
});

it("allows clearing a real project named Choose project", async () => {
  await useWorkbenchStore
    .getState()
    .createChat(useWorkbenchStore.getState().project!.id);
  const project = useWorkbenchStore.getState().project!;
  useWorkbenchStore.setState({
    agentCenter: "chat",
    newChatDraft: null,
    project: { ...project, name: "Choose project" },
  });
  render(<ProjectScopeChip projectName="Choose project" />);
  expect(
    screen.getByRole("button", { name: "Project: Choose project" }),
  ).toBeEnabled();
  fireEvent.click(
    screen.getByRole("button", { name: "Remove project from this chat" }),
  );
  await waitFor(() =>
    expect(useWorkbenchStore.getState().project?.isInternal).toBe(true),
  );
  expect(
    screen.getByRole("button", { name: "Choose project" }),
  ).toBeInTheDocument();
});

it("keeps the entire unsent composer through New project creation and reparenting", async () => {
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
  render(
    <>
      <AgentCenterKeeper windowMode="agent" />
      <ProjectScopeChip projectName="Choose project" />
    </>,
  );
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
    agentCenter: "chat",
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

it.each(["before creation", "during creation"])(
  "keeps the original private chat and unsent context when files appear %s",
  async (when) => {
    await act(async () => useWorkbenchStore.getState().createChat(null));
    const state = useWorkbenchStore.getState();
    const chat = state.activeTabId!;
    const project = state.project!;
    const projectsBefore = state.projects.length;
    const owner = state.captureChatOwnership(chat)!;
    const context = {
      kind: "browserPage" as const,
      tabId: "browser-1",
      url: "https://example.test/",
      title: "Reference",
      text: "Keep this attachment",
    };
    state.setChatComposer(owner, "Keep this unsent prompt");
    expect(state.attachChatContext(owner, context)).toBe(true);
    state.setChatHarnessSelection(owner, {
      harnessId: "sage-simulator",
      modelId: "sim-fast",
    });
    const saveFile = () =>
      getWorkbenchPorts().project.createProjectFile({
        projectId: project.id,
        projectGeneration: state.projectGeneration,
        expectedTransitionEpoch:
          useWorkbenchStore.getState().nativeTransitionEpoch,
        parent: { rootId: project.roots[0].id, relativePath: "" },
        basename: "notes.txt",
      });
    if (when === "before creation") await saveFile();
    else {
      const move = getWorkbenchPorts().chat.setChatProject.bind(
        getWorkbenchPorts().chat,
      );
      vi.spyOn(getWorkbenchPorts().chat, "setChatProject").mockImplementation(
        async (...args) => {
          // An external writer saves in the source after creation's preflight.
          const root = { rootId: project.roots[0].id, relativePath: "" };
          browserDirectories.get(resourceKey(root))!.entries.push({
            name: "notes.txt",
            resource: { ...root, relativePath: "notes.txt" },
            kind: "file",
            byteLen: 12,
          });
          return move(...args);
        },
      );
    }
    render(<ProjectScopeChip projectName="Choose project" />);
    fireEvent.click(screen.getByRole("button", { name: "Choose project" }));
    fireEvent.click(await screen.findByRole("option", { name: "New project" }));
    fireEvent.change(
      await screen.findByRole("textbox", { name: "Project name" }),
      {
        target: { value: "Destination" },
      },
    );
    fireEvent.click(screen.getByRole("button", { name: "Create project" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "This chat's folder has files. Start a new chat in the project instead.",
    );
    expect(screen.getByRole("dialog")).toBeVisible();
    expect(useWorkbenchStore.getState()).toMatchObject({
      activeTabId: chat,
      project: { id: project.id },
      composerText: "Keep this unsent prompt",
    });
    expect(useWorkbenchStore.getState().chatUiById[chat]).toMatchObject({
      composerText: "Keep this unsent prompt",
      attachedContext: [context],
      harnessId: "sage-simulator",
      modelId: "sim-fast",
    });
    expect(useWorkbenchStore.getState().projects).toHaveLength(
      projectsBefore + (when === "during creation" ? 1 : 0),
    );
    const listing = await getWorkbenchPorts().project.listDirectory({
      rootId: project.roots[0].id,
      relativePath: "",
    });
    expect(listing.entries.map((entry) => entry.name)).toEqual(["notes.txt"]);
  },
);

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

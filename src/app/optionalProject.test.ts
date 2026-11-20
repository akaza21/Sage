import { afterEach, beforeEach, expect, it } from "vitest";
import { resetBrowserDesktopForTests } from "../services/browserDesktop";
import { currentProjectLayout } from "./projectProjection";
import { useWorkbenchStore } from "./store";

const initial = useWorkbenchStore.getState();
beforeEach(async () => {
  resetBrowserDesktopForTests();
  useWorkbenchStore.setState(initial, true);
  await useWorkbenchStore.getState().initialize();
});
afterEach(() => useWorkbenchStore.setState(initial, true));

it("re-parents an empty chat without losing its prompt, captured context or harness choices", async () => {
  const state = useWorkbenchStore.getState();
  const project = state.project!.id;
  await state.createChat();
  const chat = useWorkbenchStore.getState().activeTabId!;
  const owner = useWorkbenchStore.getState().captureChatOwnership(chat)!;
  state.setChatComposer(owner, "Keep my first prompt");
  state.setChatHarnessSelection(owner, {
    harnessId: "sage-simulator",
    modelId: "sim-fast",
  });
  const context = {
    kind: "browserPage" as const,
    tabId: "browser-1",
    url: "https://example.test/",
    title: "Reference",
    text: "Captured page text",
  };
  expect(state.attachChatContext(owner, context)).toBe(true);
  await state.setChatProject(null);
  expect(useWorkbenchStore.getState()).toMatchObject({
    activeTabId: chat,
    composerText: "Keep my first prompt",
    project: { isInternal: true },
  });
  expect(useWorkbenchStore.getState().chatUiById[chat]).toMatchObject({
    harnessId: "sage-simulator",
    modelId: "sim-fast",
    attachedContext: [context],
  });
  await state.setChatProject(project);
  expect(useWorkbenchStore.getState()).toMatchObject({
    activeTabId: chat,
    composerText: "Keep my first prompt",
    project: { id: project },
  });
});

it("restores and deletes an archived project-less chat while another project is open", async () => {
  const state = useWorkbenchStore.getState();
  const real = state.project!.id;
  const created = await state.createChat(null);
  expect(created.status).toBe("confirmed");
  const privateState = useWorkbenchStore.getState();
  const project = privateState.project!.id;
  const chat = privateState.activeTabId!;
  expect(await state.archiveChat(chat)).toBe(true);
  await state.switchProject(real);
  expect(useWorkbenchStore.getState().project?.id).toBe(real);
  expect(await state.restoreChat(chat, project)).toBe(true);
  expect(useWorkbenchStore.getState().unscopedChats[0].railChats?.[0].id).toBe(
    chat,
  );
  await state.openRailChat(project, chat);
  expect(useWorkbenchStore.getState().activeTabId).toBe(chat);
  expect(await state.archiveChat(chat)).toBe(true);
  await state.switchProject(real);
  expect(await state.deleteChat(chat, project)).toBe(true);
  expect(useWorkbenchStore.getState().unscopedChats).toEqual([]);
});

it("new-chat entry points distinguish a project chat from a top-level chat", async () => {
  const state = useWorkbenchStore.getState();
  await state.openNewChatDraft(state.project!.id);
  expect(useWorkbenchStore.getState().newChatDraft?.targetProjectId).toBe(
    state.project!.id,
  );
  state.discardNewChatDraft();
  await state.openNewChatDraft();
  expect(useWorkbenchStore.getState().newChatDraft?.targetProjectId).toBeNull();
  expect(currentProjectLayout(useWorkbenchStore.getState())?.projectId).toBe(
    state.project!.id,
  );
});

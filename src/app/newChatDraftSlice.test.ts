import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { MAX_CHAT_COMPOSER_BYTES } from "../domain/chatOwnership";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { resetBrowserDesktopForTests } from "../services/browserDesktop";
import { useWorkbenchStore } from "./store";

const initial = useWorkbenchStore.getState();
const ports = getWorkbenchPorts();
beforeEach(async () => {
  resetBrowserDesktopForTests();
  useWorkbenchStore.setState(initial, true);
  await useWorkbenchStore.getState().initialize();
  await useWorkbenchStore.getState().openNewChatDraft();
});
afterEach(() => {
  vi.restoreAllMocks();
  useWorkbenchStore.setState(initial, true);
  resetBrowserDesktopForTests();
});

it("bounds draft UTF-8 text and keeps the previous chat's text independently", async () => {
  const state = useWorkbenchStore.getState();
  state.setComposerText("Previous chat text");
  const text = "é".repeat(MAX_CHAT_COMPOSER_BYTES / 2);
  state.updateNewChatDraft({ text });
  expect(useWorkbenchStore.getState().newChatDraft?.text).toBe(text);
  state.updateNewChatDraft({ text: text + "é" });
  expect(useWorkbenchStore.getState().newChatDraft?.text).toBe(text);
  expect(useWorkbenchStore.getState().newChatDraft?.error).toMatch(
    /limit|bytes|exceed/i,
  );
  await state.openNewChatDraft();
  await state.activateTab(state.activeTabId!);
  expect(useWorkbenchStore.getState().composerText).toBe("Previous chat text");
  expect(useWorkbenchStore.getState().agentCenter).toBe("chat");
});

it("keeps the draft through same-project refresh and failed switching, then discards it on successful switching", async () => {
  const state = useWorkbenchStore.getState();
  state.updateNewChatDraft({ text: "Project draft" });
  await state.createChat();
  expect(useWorkbenchStore.getState().newChatDraft?.text).toBe("Project draft");
  const snapshot = await ports.project.getBootstrapSnapshot();
  vi.spyOn(ports.project, "switchProject").mockResolvedValueOnce({
    committed: false,
    snapshot: null,
    transitionEpoch: snapshot.transitionEpoch,
    recoveryError: "Switch rejected",
  });
  await state.switchProject("missing-project");
  expect(useWorkbenchStore.getState().newChatDraft?.text).toBe("Project draft");
  const changed = {
    ...snapshot,
    project: { id: "other-project", name: "Other", roots: [] },
    tabs: [],
    archivedChats: [],
    activeTabId: null,
    transitionEpoch: snapshot.transitionEpoch + 1,
  };
  vi.mocked(ports.project.switchProject).mockResolvedValueOnce({
    committed: true,
    snapshot: changed,
    transitionEpoch: changed.transitionEpoch,
    recoveryError: null,
  });
  await state.switchProject("other-project");
  expect(useWorkbenchStore.getState().project?.id).toBe("other-project");
  expect(useWorkbenchStore.getState().newChatDraft).toBeNull();
  expect(useWorkbenchStore.getState().agentCenter).toBe("chat");
});

it("refuses delayed creation after the project ownership lifetime changes", async () => {
  const before = await ports.project.getBootstrapSnapshot();
  const state = useWorkbenchStore.getState();
  const creation = state.createChat();
  useWorkbenchStore.setState({
    chatOwnershipEpoch: state.chatOwnershipEpoch + 1,
  });
  expect(await creation).toEqual({ status: "not-created" });
  expect((await ports.project.getBootstrapSnapshot()).tabs).toEqual(
    before.tabs,
  );
});

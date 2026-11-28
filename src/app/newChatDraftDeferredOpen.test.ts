import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { resetBrowserDesktopForTests } from "../../tests/ui/simulator/browserDesktop";
import { useWorkbenchStore } from "./store";

// Kept in its own file: the draft actions module must not already be cached.
const actionsLoad = vi.hoisted(() => {
  let release!: () => void;
  const ready = new Promise<void>((resolve) => {
    release = resolve;
  });
  return { ready, release };
});
vi.mock("./newChatDraftActions", async (importOriginal) => {
  await actionsLoad.ready;
  return importOriginal();
});

const initial = useWorkbenchStore.getState();
beforeEach(async () => {
  resetBrowserDesktopForTests();
  useWorkbenchStore.setState(initial, true);
  await useWorkbenchStore.getState().initialize();
});
afterEach(() => {
  useWorkbenchStore.setState(initial, true);
  resetBrowserDesktopForTests();
});

it("drops a coalesced Home open whose actions load after another chat was selected", async () => {
  const state = useWorkbenchStore.getState();
  const previousId = state.activeTabId!;
  await state.createChat();
  expect(useWorkbenchStore.getState().activeTabId).not.toBe(previousId);

  const automatic = state.openNewChatDraft(undefined, { requestFocus: false });
  const opening = state.openNewChatDraft();
  await state.activateTab(previousId);
  expect(useWorkbenchStore.getState().activeTabId).toBe(previousId);
  actionsLoad.release();
  await automatic;
  await opening;
  expect(useWorkbenchStore.getState()).toMatchObject({
    agentCenter: "chat",
    newChatDraft: null,
    activeTabId: previousId,
  });

  await state.openNewChatDraft();
  expect(useWorkbenchStore.getState().agentCenter).toBe("draft");
  expect(useWorkbenchStore.getState().newChatDraft).not.toBeNull();
});

import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { resetBrowserDesktopForTests } from "../services/browserDesktop";
import { useWorkbenchStore } from "./store";

// Kept in its own file: the send module must not already be cached.
const chunkLoad = vi.hoisted(() => ({ fail: true }));
vi.mock("./runNewChatDraftSend", async (importOriginal) => {
  if (chunkLoad.fail)
    throw new Error("Failed to fetch dynamically imported module");
  return importOriginal();
});

const initial = useWorkbenchStore.getState();
beforeEach(async () => {
  resetBrowserDesktopForTests();
  useWorkbenchStore.setState(initial, true);
  await useWorkbenchStore.getState().initialize();
  await useWorkbenchStore.getState().openNewChatDraft();
});
afterEach(() => {
  useWorkbenchStore.setState(initial, true);
  resetBrowserDesktopForTests();
});

it("keeps the draft editable and sendable after its send module fails to load", async () => {
  const state = useWorkbenchStore.getState();
  state.updateNewChatDraft({ text: "Keep this prompt" });
  await state.sendNewChatDraft();
  expect(useWorkbenchStore.getState().pendingFirstSend).toBeNull();
  expect(useWorkbenchStore.getState().newChatDraft).toMatchObject({
    text: "Keep this prompt",
    creating: false,
    error: expect.stringMatching(/module/),
  });

  state.updateNewChatDraft({ text: "Keep this prompt, edited" });
  chunkLoad.fail = false;
  await state.sendNewChatDraft();
  expect(useWorkbenchStore.getState().newChatDraft).toMatchObject({
    text: "Keep this prompt, edited",
    creating: false,
    error: "Choose an available harness and model before sending.",
  });
});

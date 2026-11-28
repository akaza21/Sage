import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { projectNewChatDraft } from "../../app/newChatDraftSlice";
import { useWorkbenchStore } from "../../app/store";
import { resetBrowserDesktopForTests } from "../../../tests/ui/simulator/browserDesktop";
import { noteDraftFiles } from "./draftFileHandoff";

const native = vi.hoisted(() => ({
  calls: [] as { command: string; args: unknown }[],
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (command: string, args: unknown) => {
    native.calls.push({ command, args });
    return null;
  },
}));

const initial = useWorkbenchStore.getState();
const released = () =>
  native.calls
    .filter(({ command }) => command === "release_draft_prompt_files")
    .map(({ args }) => (args as { draft: { draftToken: string } }).draft);
const draft = () => useWorkbenchStore.getState().newChatDraft!;

beforeEach(async () => {
  resetBrowserDesktopForTests();
  useWorkbenchStore.setState(initial, true);
  await useWorkbenchStore.getState().initialize();
  await useWorkbenchStore.getState().openNewChatDraft();
  // From here the WebView talks to native, as in the desktop app.
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    configurable: true,
    value: {},
  });
  native.calls.length = 0;
});
afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  useWorkbenchStore.setState(initial, true);
  resetBrowserDesktopForTests();
});

it("keeps a retargeted draft's staged files and counts them as its work", async () => {
  const token = draft().fileToken!;
  noteDraftFiles(token, "revision-1");
  useWorkbenchStore.getState().updateNewChatDraft({ targetProjectId: null });
  // An explicit New chat elsewhere moves a draft holding only files, too.
  const project = useWorkbenchStore.getState().project!.id;
  await useWorkbenchStore.getState().openNewChatDraft(project);
  expect(draft()).toMatchObject({
    fileToken: token,
    fileRevision: "revision-1",
    targetProjectId: project,
  });
  await vi.waitFor(() => expect(released()).toEqual([]));
});

it("releases the draft's native slot when it is discarded or a project switch drops it", async () => {
  const first = draft().fileToken!;
  noteDraftFiles(first, "revision-1");
  useWorkbenchStore.getState().discardNewChatDraft();
  await vi.waitFor(() => expect(released()).toEqual([{ draftToken: first }]));

  await useWorkbenchStore.getState().openNewChatDraft();
  const second = draft().fileToken!;
  expect(second).not.toBe(first);
  const state = useWorkbenchStore.getState();
  useWorkbenchStore.setState(projectNewChatDraft(state, "another-project"));
  expect(useWorkbenchStore.getState().newChatDraft).toBeNull();
  await vi.waitFor(() =>
    expect(released()).toEqual([{ draftToken: first }, { draftToken: second }]),
  );
});

import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { resetBrowserDesktopForTests } from "../services/browserDesktop";
import { desktop } from "../services/desktop";
import { useWorkbenchStore } from "./store";

const initial = useWorkbenchStore.getState();
const ports = getWorkbenchPorts();
const catalog = {
  harnesses: [
    {
      id: "draft-harness",
      name: "Draft harness",
      description: "Fixture ACP harness",
      source: "builtIn" as const,
      transport: "acp" as const,
      installState: "available" as const,
      bindingId: "draft-binding",
      accountModelDiscovery: false,
    },
  ],
  selectedHarnessId: "draft-harness",
  selectedModelId: "model-a",
};

function deferred() {
  let resolve!: () => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<void>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

let load: ReturnType<typeof deferred>;
let loading: ReturnType<typeof deferred>;
beforeEach(async () => {
  load = deferred();
  loading = deferred();
  vi.doMock("./runNewChatDraftSend", async (importOriginal) => {
    loading.resolve();
    await load.promise;
    return importOriginal();
  });
  resetBrowserDesktopForTests();
  useWorkbenchStore.setState(initial, true);
  const snapshot = ports.project.getBootstrapSnapshot.bind(ports.project);
  vi.spyOn(ports.project, "getBootstrapSnapshot").mockImplementation(
    async () => ({
      ...(await snapshot()),
      ...catalog,
    }),
  );
  vi.spyOn(desktop, "listHarnessModels").mockResolvedValue([
    { id: "model-a", name: "Model A", isDefault: true },
  ]);
  await useWorkbenchStore.getState().initialize();
  await useWorkbenchStore.getState().openNewChatDraft();
});
afterEach(() => {
  vi.doUnmock("./runNewChatDraftSend");
  vi.restoreAllMocks();
  useWorkbenchStore.setState(initial, true);
  resetBrowserDesktopForTests();
});

it("unlocks a retained draft when navigation cancels a send whose module then fails to load", async () => {
  const before = await ports.project.getBootstrapSnapshot();
  const state = useWorkbenchStore.getState();
  state.updateNewChatDraft({ text: "Keep this cancelled prompt" });
  const sending = state.sendNewChatDraft();
  await loading.promise;
  await state.activateTab(before.activeTabId!);
  expect(useWorkbenchStore.getState().pendingFirstSend).toBeNull();
  load.reject(new Error("Failed to fetch dynamically imported module"));
  await sending;

  await state.openNewChatDraft();
  expect(useWorkbenchStore.getState().newChatDraft).toMatchObject({
    text: "Keep this cancelled prompt",
    creating: false,
  });
  state.updateNewChatDraft({ text: "Edited after cancellation" });
  expect(useWorkbenchStore.getState().newChatDraft?.text).toBe(
    "Edited after cancellation",
  );
  expect((await ports.project.getBootstrapSnapshot()).tabs).toEqual(
    before.tabs,
  );

  vi.doUnmock("./runNewChatDraftSend");
  await state.sendNewChatDraft();
  expect(useWorkbenchStore.getState().newChatDraft).toBeNull();
  expect(useWorkbenchStore.getState().composerText).toBe(
    "Edited after cancellation",
  );
});

it("cancels deferred sending after failed project switching recovers the original project", async () => {
  const before = await ports.project.getBootstrapSnapshot();
  const state = useWorkbenchStore.getState();
  state.updateNewChatDraft({ text: "Send only after I retry" });
  const sending = state.sendNewChatDraft();
  await loading.promise;
  vi.spyOn(ports.project, "switchProject").mockRejectedValueOnce(
    new Error("Switch failed"),
  );
  await state.switchProject("missing-project");
  expect(useWorkbenchStore.getState().project?.id).toBe(before.project!.id);
  expect(useWorkbenchStore.getState().chatOwnershipEpoch).toBeGreaterThan(
    state.chatOwnershipEpoch,
  );
  load.resolve();
  await sending;

  expect((await ports.project.getBootstrapSnapshot()).tabs).toEqual(
    before.tabs,
  );
  expect(useWorkbenchStore.getState().pendingFirstSend).toBeNull();
  expect(useWorkbenchStore.getState().newChatDraft).toMatchObject({
    text: "Send only after I retry",
    creating: false,
  });
  state.updateNewChatDraft({ text: "Retry after recovery" });
  await state.sendNewChatDraft();
  expect(useWorkbenchStore.getState().newChatDraft).toBeNull();
  expect(useWorkbenchStore.getState().composerText).toBe(
    "Retry after recovery",
  );
  expect((await ports.project.getBootstrapSnapshot()).tabs).toHaveLength(
    before.tabs.length + 1,
  );
});

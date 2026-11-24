import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { resetBrowserDesktopForTests } from "../services/browserDesktop";
import { useWorkbenchStore } from "./store";

const initial = useWorkbenchStore.getState();
const store = () => useWorkbenchStore.getState();
let real: string;
let realChat: string;
let first: { project: string; chat: string };
let second: { project: string; chat: string };

async function privateChat() {
  await store().createChat(null);
  const chat = { project: store().project!.id, chat: store().activeTabId! };
  await store().switchProject(real);
  return chat;
}

/** Holds the model discovery that keeps a Project switch transitioning. */
function holdDiscovery() {
  const discover = store().loadHarnessModelsForProjection;
  let release!: () => void;
  let entered!: () => void;
  const gate = new Promise<void>((resolve) => (release = resolve));
  const started = new Promise<void>((resolve) => (entered = resolve));
  useWorkbenchStore.setState({
    loadHarnessModelsForProjection: async (...args) => {
      entered();
      await gate;
      return discover(...args);
    },
  });
  return { started, release };
}

async function settled() {
  await vi.waitFor(() => expect(store().projectTransitioning).toBe(false));
  // The replay runs on the microtask after the release edge.
  await new Promise((resolve) => setTimeout(resolve, 0));
  await vi.waitFor(() => expect(store().projectTransitioning).toBe(false));
}

beforeEach(async () => {
  resetBrowserDesktopForTests();
  useWorkbenchStore.setState(initial, true);
  await store().initialize();
  real = store().project!.id;
  realChat = store().tabs.find((tab) => tab.kind === "chat")!.id;
  first = await privateChat();
  second = await privateChat();
});
afterEach(() => {
  vi.restoreAllMocks();
  useWorkbenchStore.setState(initial, true);
});

it("opens a chat clicked during a project switch once the switch settles", async () => {
  const held = holdDiscovery();
  const switching = store().openRailChat(first.project, first.chat);
  await held.started;
  expect(store().projectTransitioning).toBe(true);
  await store().openRailChat(second.project, second.chat);
  expect(store().project?.id).toBe(first.project);
  expect(store().activeTabId).not.toBe(second.chat);
  held.release();
  await switching;
  await settled();
  expect(store()).toMatchObject({
    project: { id: second.project },
    activeTabId: second.chat,
    error: null,
  });
});

it("opens only the latest chat clicked during a project switch", async () => {
  const switches = vi.spyOn(getWorkbenchPorts().project, "switchProject");
  const held = holdDiscovery();
  const switching = store().openRailChat(first.project, first.chat);
  await held.started;
  await store().openRailChat(second.project, second.chat);
  await store().openRailChat(real, realChat);
  held.release();
  await switching;
  await settled();
  expect(store()).toMatchObject({
    project: { id: real },
    activeTabId: realChat,
    error: null,
  });
  expect(
    switches.mock.calls.map(([request]) => request.targetProjectId),
  ).toEqual([first.project, real]);
});

it("drops a held open whose chat was archived before the switch settled", async () => {
  const switches = vi.spyOn(getWorkbenchPorts().project, "switchProject");
  const held = holdDiscovery();
  const switching = store().openRailChat(first.project, first.chat);
  await held.started;
  await store().openRailChat(second.project, second.chat);
  // The settling projection reports the held chat as archived.
  useWorkbenchStore.setState((state) => ({
    unscopedChats: state.unscopedChats.map((item) =>
      item.id === second.project
        ? {
            ...item,
            railChats: [],
            railArchived: item.railChats!.map(({ id, title }) => ({
              id,
              title,
            })),
          }
        : item,
    ),
  }));
  held.release();
  await switching;
  await settled();
  expect(store()).toMatchObject({
    project: { id: first.project },
    activeTabId: first.chat,
    pendingChatOpen: null,
    error: null,
  });
  expect(switches).toHaveBeenCalledTimes(1);
});

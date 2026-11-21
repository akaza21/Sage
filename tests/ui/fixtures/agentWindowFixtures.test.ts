import { afterEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../../../src/app/store";
import { getWorkbenchPorts } from "../../../src/ports/workbenchRuntime";
import { currentProjectLayout } from "../../../src/app/projectProjection";
import { acpDesktop } from "../../../src/services/acpDesktop";
import { resetBrowserDesktopForTests } from "../../../src/services/browserDesktop";
import { installWindowNewChatFixture } from "./agentWindowFixtures";

const initial = useWorkbenchStore.getState();
const getChatPage = acpDesktop.getChatPage;
const createChat = getWorkbenchPorts().chat.createChat;

afterEach(() => {
  acpDesktop.getChatPage = getChatPage;
  getWorkbenchPorts().chat.createChat = createChat;
  useWorkbenchStore.setState(initial, true);
  vi.restoreAllMocks();
});

it.each([null, "current"])(
  "creates the requested %s scope with the caller's Chat identity",
  async (scope) => {
    resetBrowserDesktopForTests();
    await useWorkbenchStore.getState().initialize();
    const state = useWorkbenchStore.getState();
    const project = state.project!;
    const layout = currentProjectLayout(state)!;
    installWindowNewChatFixture();
    const target = scope === null ? null : project.id;
    const result = await getWorkbenchPorts().chat.createChat(
      layout,
      state.nativeTransitionEpoch,
      target,
      "window-created-chat",
    );
    expect(result.committed).toBe(true);
    expect(result.snapshot.activeTabId).toBe("window-created-chat");
    expect(result.snapshot.sessionId).toBeNull();
    expect(result.snapshot.project?.isInternal === true).toBe(scope === null);
    if (scope === null) {
      expect(result.snapshot.project?.id).not.toBe(project.id);
      expect(result.snapshot.tabs.map((tab) => tab.id)).toEqual([
        "window-created-chat",
      ]);
      expect(result.snapshot.projects.map((item) => item.id)).toContain(
        project.id,
      );
    } else expect(result.snapshot.project?.id).toBe(project.id);
    const page = await acpDesktop.getChatPage({
      projectId: result.snapshot.project!.id as never,
      chatId: "window-created-chat" as never,
      cursor: null,
    });
    expect(page).toMatchObject({
      projectId: result.snapshot.project!.id,
      chatId: "window-created-chat",
      sessions: [],
      turns: [],
    });
  },
);

import { afterEach, beforeEach, expect, it } from "vitest";
import type { WorkbenchTab } from "../domain/workbench";
import { openNotifiedChat } from "./openChat";
import { useWorkbenchStore } from "./store";

const initial = useWorkbenchStore.getState();
const chatTab = (id: string): WorkbenchTab => ({
  id,
  kind: "chat",
  title: id,
  position: 0,
  pinned: false,
  dirty: false,
  status: "idle",
});

// Store transitions are native round trips; these stand-ins apply only the
// layout each one commits, so the routing between them is what is tested.
beforeEach(() => {
  useWorkbenchStore.setState({
    project: { id: "active", name: "Active", roots: [] },
    tabs: [],
    activeTabId: null,
    archivedChats: [{ id: "closed", title: "Closed" }],
    projects: [
      {
        id: "other",
        name: "Other",
        rootCount: 1,
        lastOpenedAt: 1,
        railChats: [],
        railArchived: [{ id: "archived", title: "Archived" }],
      },
    ],
    restoreChat: async (chatId, projectId) => {
      const state = useWorkbenchStore.getState();
      if (projectId === state.project?.id) {
        useWorkbenchStore.setState({
          tabs: [...state.tabs, chatTab(chatId)],
          activeTabId: chatId,
          archivedChats: [],
        });
      } else {
        const restored = (folder: (typeof state.projects)[number]) =>
          folder.id === projectId
            ? {
                ...folder,
                railArchived: [],
                railChats: [
                  { id: chatId, title: chatId, pinned: false, position: 0 },
                ],
              }
            : folder;
        useWorkbenchStore.setState({
          unscopedChats: state.unscopedChats.map(restored),
          projects: state.projects.map(restored),
        });
      }
      return true;
    },
    openRailChat: async (projectId, chatId) => {
      const folder = useWorkbenchStore.getState();
      const owner = [...folder.projects, ...folder.unscopedChats].find(
        (item) => item.id === projectId,
      );
      if (!owner?.railChats?.some((chat) => chat.id === chatId)) return;
      useWorkbenchStore.setState({
        project: { id: projectId, name: owner.name, roots: [] },
        tabs: [chatTab(chatId)],
        activeTabId: chatId,
      });
    },
  });
});
afterEach(() => useWorkbenchStore.setState(initial, true));

it("restores a closed chat in the active folder and shows it", async () => {
  await openNotifiedChat("active", "closed");
  const state = useWorkbenchStore.getState();
  expect(state.activeTabId).toBe("closed");
  expect(state.archivedChats).toEqual([]);
});

it("restores an archived chat in another folder and opens that folder", async () => {
  await openNotifiedChat("other", "archived");
  const state = useWorkbenchStore.getState();
  expect(state.project?.id).toBe("other");
  expect(state.activeTabId).toBe("archived");
});

it("reopens an inactive archived private chat from its notification", async () => {
  useWorkbenchStore.setState({
    unscopedChats: [
      {
        id: "private",
        name: "",
        rootCount: 1,
        lastOpenedAt: 1,
        railArchived: [{ id: "private-archived", title: "Private archived" }],
        railChats: [],
      },
    ],
  });
  await openNotifiedChat("private", "private-archived");
  expect(useWorkbenchStore.getState().project?.id).toBe("private");
  expect(useWorkbenchStore.getState().activeTabId).toBe("private-archived");
  expect(useWorkbenchStore.getState().unscopedChats[0].railArchived).toEqual(
    [],
  );
});

it("leaves the workbench alone for a chat that no longer exists", async () => {
  await openNotifiedChat("active", "deleted");
  const state = useWorkbenchStore.getState();
  expect(state.project?.id).toBe("active");
  expect(state.activeTabId).toBeNull();
});

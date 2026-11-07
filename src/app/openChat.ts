import type {
  ArchivedChatSummary,
  ProjectListItem,
  ProjectSummary,
  WorkbenchTab,
} from "../domain/workbench";
import { useWorkbenchStore } from "./store";

export interface ChatPlacement {
  /** The chat has a tab in its project's layout. */
  attached: boolean;
  /** The chat is closed or archived and can be restored. */
  archived: boolean;
}

/** Where a chat lives, from the active layout or its folder's rail. */
export function chatPlacement(
  state: {
    project: ProjectSummary | null;
    tabs: ReadonlyArray<WorkbenchTab>;
    archivedChats: ReadonlyArray<ArchivedChatSummary>;
    projects: ReadonlyArray<ProjectListItem>;
  },
  projectId: string,
  chatId: string,
): ChatPlacement {
  if (state.project?.id === projectId) {
    return {
      attached: state.tabs.some(
        (tab) => tab.kind === "chat" && tab.id === chatId,
      ),
      archived: state.archivedChats.some((chat) => chat.id === chatId),
    };
  }
  const folder = state.projects.find((item) => item.id === projectId);
  return {
    attached: Boolean(folder?.railChats?.some((chat) => chat.id === chatId)),
    archived: Boolean(folder?.railArchived?.some((chat) => chat.id === chatId)),
  };
}

/**
 * Opens a chat from a search result or a notification. An attached chat is
 * activated, switching folders when it belongs to another one; a closed or
 * archived chat is restored. A chat restored into another folder stays there
 * unless `revealRestored` asks to open that folder too.
 */
export async function openChat(
  projectId: string,
  chatId: string,
  { attached, revealRestored }: { attached: boolean; revealRestored: boolean },
) {
  const store = useWorkbenchStore.getState();
  if (!attached) {
    const restored = await store.restoreChat(chatId, projectId);
    if (!restored || !revealRestored) return;
    if (useWorkbenchStore.getState().project?.id === projectId) return;
  }
  if (projectId === useWorkbenchStore.getState().project?.id) {
    await useWorkbenchStore.getState().activateChat(chatId);
  } else {
    await useWorkbenchStore.getState().openRailChat(projectId, chatId);
  }
}

/**
 * Opens the chat a notification names. Rails list a bounded set of attached
 * chats, so only a chat known to be closed or archived is restored, and a
 * restored chat is always shown, in its own folder.
 */
export async function openNotifiedChat(projectId: string, chatId: string) {
  const { archived } = chatPlacement(
    useWorkbenchStore.getState(),
    projectId,
    chatId,
  );
  await openChat(projectId, chatId, {
    attached: !archived,
    revealRestored: true,
  });
}

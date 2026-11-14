import type {
  ArchivedChatSummary,
  ProjectListItem,
  ProjectSummary,
  WorkbenchTab,
} from "../domain/workbench";
import { useWorkbenchStore } from "./store";

export interface ChatPlacement {
  attached: boolean;
  archived: boolean;
}

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
 * Rails list a bounded set of attached chats, so only a chat known to be closed
 * or archived is restored.
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

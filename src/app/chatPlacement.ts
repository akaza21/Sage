import type {
  ArchivedChatSummary,
  ProjectListItem,
  ProjectSummary,
  WorkbenchTab,
} from "../domain/workbench";

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
    unscopedChats?: ReadonlyArray<ProjectListItem>;
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
  const folder =
    state.projects.find((item) => item.id === projectId) ??
    state.unscopedChats?.find((item) => item.id === projectId);
  return {
    attached: Boolean(folder?.railChats?.some((chat) => chat.id === chatId)),
    archived: Boolean(folder?.railArchived?.some((chat) => chat.id === chatId)),
  };
}

import type {
  ArchivedChatSummary,
  ProjectListItem,
  ProjectSummary,
  WorkbenchTab,
} from "../../domain/workbench";
import { MAX_AGENT_RAIL_PROJECTS } from "../../workbench/agentChatRail";
import { CHAT_SEARCH_ALL_FOLDERS_ID } from "./chatSearchFanout";

export interface ChatSearchScope {
  activeProjectId: string;
  searchProjectId: string;
  projectGeneration: number;
  transitionEpoch: number;
}

export interface ChatSearchFolderOption {
  id: string;
  name: string;
}

export function admitChatSearchFolder(
  folders: ReadonlyArray<ChatSearchFolderOption>,
  requestedId: string | undefined,
  fallbackId: string,
): string {
  if (requestedId && folders.some((folder) => folder.id === requestedId)) {
    return requestedId;
  }
  return fallbackId;
}

let requestedFolderId: string | null = null;
let requestedFocusReturn: (() => void) | null = null;
const folderRequestListeners = new Set<() => void>();

export function requestChatSearchInFolder(
  projectId: string,
  focusReturn?: () => void,
) {
  if (
    typeof projectId !== "string" ||
    projectId.length === 0 ||
    projectId.length > 512
  ) {
    return;
  }
  requestedFolderId = projectId;
  requestedFocusReturn = focusReturn ?? null;
  folderRequestListeners.forEach((listener) => listener());
}

export function takeChatSearchFolderRequest(): string | null {
  const next = requestedFolderId;
  requestedFolderId = null;
  return next;
}

export function takeChatSearchFocusReturn(): (() => void) | null {
  const next = requestedFocusReturn;
  requestedFocusReturn = null;
  return next;
}

export function subscribeChatSearchFolderRequest(listener: () => void) {
  folderRequestListeners.add(listener);
  return () => {
    folderRequestListeners.delete(listener);
  };
}

export function clearChatSearchFolderRequest() {
  requestedFolderId = null;
  requestedFocusReturn = null;
}

export function chatSearchFolderOptions(
  projects: ReadonlyArray<{ id: string; name: string }>,
): ChatSearchFolderOption[] {
  const next: ChatSearchFolderOption[] = [];
  const limit = Math.min(projects.length, MAX_AGENT_RAIL_PROJECTS);
  for (let index = 0; index < limit; index += 1) {
    const project = projects[index];
    if (
      !project ||
      typeof project.id !== "string" ||
      project.id.length === 0 ||
      project.id.length > 512
    ) {
      continue;
    }
    const name =
      typeof project.name === "string" && project.name.length > 0
        ? project.name.slice(0, 64)
        : "Project";
    next.push({ id: project.id, name });
  }
  if (next.length === 0) return next;
  return [{ id: CHAT_SEARCH_ALL_FOLDERS_ID, name: "All folders" }, ...next];
}

export function matchesChatSearchScope(
  state: {
    project: ProjectSummary | null;
    projectGeneration: number;
    nativeTransitionEpoch: number;
    projectTransitioning: boolean;
  },
  scope: ChatSearchScope,
) {
  return (
    !state.projectTransitioning &&
    state.project?.id === scope.activeProjectId &&
    state.projectGeneration === scope.projectGeneration &&
    state.nativeTransitionEpoch === scope.transitionEpoch
  );
}

export function chatSearchAttachment(
  state: {
    project: ProjectSummary | null;
    tabs: ReadonlyArray<WorkbenchTab>;
    archivedChats: ReadonlyArray<ArchivedChatSummary>;
    projects: ReadonlyArray<ProjectListItem>;
  },
  projectId: string,
  chatId: string,
): { attached: boolean; archived: boolean } {
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

import type { ProjectChatSearchResult } from "./chatSearchDomain";
import type { ProjectSummary } from "../../domain/workbench";
import { MAX_AGENT_RAIL_PROJECTS } from "../../domain/agentRailLimits";
import { projectLabel } from "../../domain/project";
import {
  CHAT_SEARCH_ALL_FOLDERS_ID,
  type ChatSearchFolderOption,
  type ChatSearchScope,
} from "./chatSearchScope";

export { CHAT_SEARCH_ALL_FOLDERS_ID };
export const MAX_CHAT_SEARCH_FANOUT_FOLDERS = 8;

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

export function chatSearchFolderOptions(
  projects: ReadonlyArray<{ id: string; name: string }>,
  hasPrivateChats = false,
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
    const name = projectLabel(project.name);
    next.push({ id: project.id, name });
  }
  if (next.length === 0 && !hasPrivateChats) return next;
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

export interface ChatSearchFanoutPick extends ProjectChatSearchResult {
  projectId: string;
}

export interface ChatSearchFanoutView {
  query: string;
  results: ChatSearchFanoutPick[];
  hasMore: boolean;
}

export function isChatSearchFanout(id: string) {
  return id === CHAT_SEARCH_ALL_FOLDERS_ID;
}

export function chatSearchResultFolderName(
  folders: ReadonlyArray<{ id: string; name: string }>,
  projectId: string,
): string | null {
  if (isChatSearchFanout(projectId)) return null;
  for (const folder of folders) {
    if (folder.id === projectId && !isChatSearchFanout(folder.id)) {
      return folder.name;
    }
  }
  return null;
}

export function chatSearchFanoutFolderIds(
  options: ReadonlyArray<{ id: string }>,
): string[] {
  const next: string[] = [];
  for (const folder of options) {
    if (
      typeof folder.id !== "string" ||
      folder.id.length === 0 ||
      folder.id.length > 512 ||
      isChatSearchFanout(folder.id)
    ) {
      continue;
    }
    next.push(folder.id);
    if (next.length >= MAX_CHAT_SEARCH_FANOUT_FOLDERS) break;
  }
  return next;
}

export const chatSearchResultKey = (result: ChatSearchFanoutPick) =>
  `${result.projectId}:${result.chatId}`;

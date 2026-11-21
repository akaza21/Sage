import {
  MAX_CHAT_SEARCH_RESULTS,
  validateProjectChatSearchRequest,
  type ProjectChatSearchGateway,
  type ProjectChatSearchRequest,
  type ProjectChatSearchResponse,
} from "./chatSearchDomain";
import { MAX_AGENT_RAIL_PROJECTS } from "../../domain/agentRailLimits";
import {
  MAX_CHAT_SEARCH_FANOUT_FOLDERS,
  chatSearchFanoutFolderIds,
  isChatSearchFanout,
  type ChatSearchFanoutPick,
  type ChatSearchFanoutView,
} from "./chatSearchFanout";
function mergePage(
  current: ChatSearchFanoutPick[],
  page: ProjectChatSearchResponse,
) {
  const candidates = [
    ...current,
    ...page.results.map((result) => ({ ...result, projectId: page.projectId })),
  ];
  candidates.sort(
    (left, right) => (right.updatedAt ?? 0) - (left.updatedAt ?? 0),
  );
  return {
    results: candidates.slice(0, MAX_CHAT_SEARCH_RESULTS),
    hasMore: page.hasMore || candidates.length > MAX_CHAT_SEARCH_RESULTS,
  };
}

export function mergeChatSearchFanout(
  pages: ReadonlyArray<ProjectChatSearchResponse>,
  visited: number,
  listed: number,
) {
  let results: ChatSearchFanoutPick[] = [];
  let hasMore = visited < listed;
  for (const page of pages) {
    const merged = mergePage(results, page);
    results = merged.results;
    hasMore ||= merged.hasMore;
  }
  return { results, hasMore };
}

export async function searchChatFolders(args: {
  folderIds: ReadonlyArray<string>;
  privateFolderIds?: ReadonlyArray<string>;
  listedCount: number;
  projectGeneration: number;
  expectedTransitionEpoch: number;
  query: string;
  isCurrent?: () => boolean;
  search: ProjectChatSearchGateway["search"];
}): Promise<{ results: ChatSearchFanoutPick[]; hasMore: boolean }> {
  let results: ChatSearchFanoutPick[] = [];
  let hasMore = false;
  let visited = 0;
  // Ownership comes from native unscopedChats, independently of public previews.
  const folderIds = [
    ...args.folderIds.slice(0, MAX_CHAT_SEARCH_FANOUT_FOLDERS),
    ...(args.privateFolderIds ?? []).slice(0, MAX_AGENT_RAIL_PROJECTS),
  ];
  for (const projectId of new Set(folderIds)) {
    if (isChatSearchFanout(projectId)) continue;
    if (args.isCurrent?.() === false) throw new Error("Chat search cancelled");
    const request: ProjectChatSearchRequest = {
      projectId,
      projectGeneration: args.projectGeneration,
      expectedTransitionEpoch: args.expectedTransitionEpoch,
      query: args.query,
    };
    validateProjectChatSearchRequest(request);
    const page = await args.search(request);
    visited += 1;
    const merged = mergePage(results, page);
    results = merged.results;
    hasMore ||= merged.hasMore;
  }
  if (visited === 0) {
    throw new Error("Project chat search requires a valid project ID");
  }
  return { results, hasMore: hasMore || visited < args.listedCount };
}

export async function runScopedChatSearch(args: {
  searchProjectId: string;
  folders: ReadonlyArray<{ id: string }>;
  privateFolders?: ReadonlyArray<{ id: string }>;
  projectGeneration: number;
  expectedTransitionEpoch: number;
  query: string;
  isCurrent?: () => boolean;
  search: ProjectChatSearchGateway["search"];
}): Promise<ChatSearchFanoutView> {
  if (isChatSearchFanout(args.searchProjectId)) {
    const folderIds = chatSearchFanoutFolderIds(args.folders);
    let listedCount = 0;
    for (const folder of args.folders) {
      if (!isChatSearchFanout(folder.id)) listedCount += 1;
    }
    const merged = await searchChatFolders({
      folderIds,
      privateFolderIds: args.privateFolders
        ?.slice(0, MAX_AGENT_RAIL_PROJECTS)
        .map((folder) => folder.id),
      listedCount: listedCount + (args.privateFolders?.length ?? 0),
      projectGeneration: args.projectGeneration,
      expectedTransitionEpoch: args.expectedTransitionEpoch,
      query: args.query,
      isCurrent: args.isCurrent,
      search: args.search,
    });
    return {
      query: args.query,
      results: merged.results,
      hasMore: merged.hasMore,
    };
  }
  const request: ProjectChatSearchRequest = {
    projectId: args.searchProjectId,
    projectGeneration: args.projectGeneration,
    expectedTransitionEpoch: args.expectedTransitionEpoch,
    query: args.query,
  };
  validateProjectChatSearchRequest(request);
  const page = await args.search(request);
  return {
    query: page.query,
    results: page.results.map((result) => ({
      ...result,
      projectId: page.projectId,
    })),
    hasMore: page.hasMore,
  };
}

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
export function mergeChatSearchFanout(
  pages: ReadonlyArray<ProjectChatSearchResponse>,
  visited: number,
  listed: number,
): { results: ChatSearchFanoutPick[]; hasMore: boolean } {
  const results: ChatSearchFanoutPick[] = [];
  let hasMore = visited < listed;
  for (const page of pages) {
    if (page.hasMore) hasMore = true;
    for (const result of page.results) {
      if (results.length >= MAX_CHAT_SEARCH_RESULTS) {
        return { results, hasMore: true };
      }
      results.push({ ...result, projectId: page.projectId });
    }
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
  search: ProjectChatSearchGateway["search"];
}): Promise<{ results: ChatSearchFanoutPick[]; hasMore: boolean }> {
  const pages: ProjectChatSearchResponse[] = [];
  let visited = 0;
  // Ownership comes from native unscopedChats, independently of public previews.
  const folderIds = [
    ...args.folderIds.slice(0, MAX_CHAT_SEARCH_FANOUT_FOLDERS),
    ...(args.privateFolderIds ?? []).slice(0, MAX_AGENT_RAIL_PROJECTS),
  ];
  for (const projectId of new Set(folderIds)) {
    if (isChatSearchFanout(projectId)) continue;
    const request: ProjectChatSearchRequest = {
      projectId,
      projectGeneration: args.projectGeneration,
      expectedTransitionEpoch: args.expectedTransitionEpoch,
      query: args.query,
    };
    validateProjectChatSearchRequest(request);
    const page = await args.search(request);
    pages.push(page);
    visited += 1;
    const merged = mergeChatSearchFanout(pages, visited, args.listedCount);
    if (merged.results.length >= MAX_CHAT_SEARCH_RESULTS) {
      return merged;
    }
  }
  if (pages.length === 0) {
    throw new Error("Project chat search requires a valid project ID");
  }
  return mergeChatSearchFanout(pages, visited, args.listedCount);
}

export async function runScopedChatSearch(args: {
  searchProjectId: string;
  folders: ReadonlyArray<{ id: string }>;
  privateFolders?: ReadonlyArray<{ id: string }>;
  projectGeneration: number;
  expectedTransitionEpoch: number;
  query: string;
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

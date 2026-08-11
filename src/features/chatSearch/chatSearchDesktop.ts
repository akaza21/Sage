import {
  parseProjectChatSearchResponse,
  validateProjectChatSearchRequest,
  type ProjectChatSearchGateway,
  type ProjectChatSearchRequest,
} from "./chatSearchDomain";

export type ProjectChatSearchInvoke = (
  command: string,
  args: Record<string, unknown>,
) => Promise<unknown>;

export function createProjectChatSearchAdapter(
  invoke: ProjectChatSearchInvoke,
): ProjectChatSearchGateway {
  return {
    async search(request: ProjectChatSearchRequest) {
      const normalized = { ...request, query: request.query.trim() };
      validateProjectChatSearchRequest(normalized);
      const response = parseProjectChatSearchResponse(
        await invoke("search_project_chats", { request: normalized }),
      );
      if (
        response.projectId !== normalized.projectId ||
        response.projectGeneration !== normalized.projectGeneration ||
        response.transitionEpoch !== normalized.expectedTransitionEpoch ||
        response.query !== normalized.query
      ) {
        throw new Error("Project Chat search returned another intent identity");
      }
      return response;
    },
  };
}

async function invokeProjectChatSearch(
  command: string,
  args: Record<string, unknown>,
): Promise<unknown> {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
    throw new Error("Chat search is available in the Sage desktop runtime.");
  }
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke(command, args);
}

export const projectChatSearchDesktop = createProjectChatSearchAdapter(
  invokeProjectChatSearch,
);

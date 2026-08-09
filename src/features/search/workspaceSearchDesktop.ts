import {
  parseWorkspaceSearchResponse,
  validateWorkspaceSearchRequest,
  type WorkspaceSearchGateway,
  type WorkspaceSearchRequest,
} from "./workspaceSearchDomain";

export type WorkspaceSearchInvoke = (
  command: string,
  args: Record<string, unknown>,
) => Promise<unknown>;

function abortError(): DOMException {
  return new DOMException("workspace Search was cancelled", "AbortError");
}

function throwIfAborted(signal?: AbortSignal): void {
  if (signal?.aborted) throw abortError();
}

export function createWorkspaceSearchAdapter(
  invoke: WorkspaceSearchInvoke,
): WorkspaceSearchGateway {
  return {
    async search(request: WorkspaceSearchRequest, signal?: AbortSignal) {
      const normalized = { ...request, query: request.query.trim() };
      validateWorkspaceSearchRequest(normalized);
      throwIfAborted(signal);
      const response = parseWorkspaceSearchResponse(
        await invoke("search_workspace", { request: normalized }),
      );
      throwIfAborted(signal);
      if (
        response.projectId !== normalized.projectId ||
        response.query !== normalized.query
      ) {
        throw new Error("invalid workspace Search response");
      }
      return response;
    },
  };
}

async function invokeWorkspaceSearch(
  command: string,
  args: Record<string, unknown>,
): Promise<unknown> {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
    throw new Error(
      "Workspace Search is available in the Sage desktop runtime.",
    );
  }
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke(command, args);
}

export const workspaceSearchDesktop = createWorkspaceSearchAdapter(
  invokeWorkspaceSearch,
);

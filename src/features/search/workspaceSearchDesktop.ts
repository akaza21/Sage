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
      const requestId = crypto.randomUUID();
      const nativeRequest = { ...normalized, requestId };
      const cancel = () => {
        void invoke("cancel_workspace_search", { requestId }).catch(() => {});
      };
      signal?.addEventListener("abort", cancel, { once: true });
      try {
        const response = parseWorkspaceSearchResponse(
          await invoke("search_workspace", { request: nativeRequest }),
        );
        throwIfAborted(signal);
        if (
          response.requestId !== requestId ||
          response.projectId !== normalized.projectId ||
          response.transitionEpoch !== normalized.expectedTransitionEpoch ||
          response.query !== normalized.query
        ) {
          throw new Error("invalid workspace Search response");
        }
        return response;
      } finally {
        signal?.removeEventListener("abort", cancel);
      }
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

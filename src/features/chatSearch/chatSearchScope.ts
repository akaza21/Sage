/**
 * The eager half of Chat search: shared limits, scope types and the request
 * bus that the rail and title bar use to open the lazy search dialog
 * (ChatSearchControl.tsx). Everything the dialog needs lives in that chunk.
 */
export const MAX_CHAT_SEARCH_QUERY_BYTES = 256;
export const CHAT_SEARCH_ALL_FOLDERS_ID = "all-folders";

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

/** Whether a folder request is waiting for the dialog to take it. */
export function hasChatSearchFolderRequest(): boolean {
  return requestedFolderId !== null;
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

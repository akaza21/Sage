import type { StateCreator } from "zustand";

import { errorMessage } from "../domain/errors";
import type * as ArchiveActions from "./chatArchiveActions";
import type { WorkbenchState } from "./store";

export type ChatRowAction = "archive" | "restore" | "delete";

export interface ChatRowError {
  projectId: string;
  chatId: string;
  action: ChatRowAction;
  message: string;
}

export type ChatActionReport = "row" | "workbench";

export interface ChatArchiveSlice {
  chatRowError: ChatRowError | null;
  archiveChat: (
    chatId: string,
    projectId?: string,
    report?: ChatActionReport,
  ) => Promise<boolean>;
  restoreChat: (
    chatId: string,
    projectId?: string,
    report?: ChatActionReport,
  ) => Promise<boolean>;
  /** Permanent; callers confirm first. */
  deleteChat: (chatId: string, projectId?: string) => Promise<boolean>;
  dismissChatRowError: () => void;
}

/** Loaded with the first action used, outside the initial bundle. */
export const createChatArchiveSlice: StateCreator<
  WorkbenchState,
  [],
  [],
  ChatArchiveSlice
> = (set, get) => {
  let actions: Promise<
    ReturnType<typeof ArchiveActions.createChatArchiveActions>
  > | null = null;
  const load = () =>
    (actions ??= import("./chatArchiveActions").then((module) =>
      module.createChatArchiveActions(set, get),
    ));
  const run = (
    projectId: string | undefined,
    action: (loaded: Awaited<ReturnType<typeof load>>) => Promise<boolean>,
  ) => {
    // An action with no Project of its own runs against the current one, so
    // it is dropped if the Project changes while the actions load: it would
    // run against the next Project. An action naming its Project runs there.
    const startedIn = get().projectGeneration;
    return load()
      .then(
        (loaded) =>
          (!!projectId || get().projectGeneration === startedIn) &&
          action(loaded),
      )
      .catch((error: unknown) => {
        actions = null;
        set({ error: errorMessage(error) });
        return false;
      });
  };

  return {
    chatRowError: null,
    dismissChatRowError: () => set({ chatRowError: null }),
    archiveChat: (chatId, projectId, report) =>
      run(projectId, (loaded) => loaded.archiveChat(chatId, projectId, report)),
    restoreChat: (chatId, projectId, report) =>
      run(projectId, (loaded) => loaded.restoreChat(chatId, projectId, report)),
    deleteChat: (chatId, projectId) =>
      run(projectId, (loaded) => loaded.deleteChat(chatId, projectId)),
  };
};

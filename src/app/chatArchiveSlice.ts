import type { StateCreator } from "zustand";

import { errorMessage } from "../domain/errors";
import type * as ArchiveActions from "./chatArchiveActions";
import type { WorkbenchState } from "./store";

/** A Chat action started from a rail or archived-list row. */
export type ChatRowAction = "archive" | "restore" | "delete";

/** A row action the backend refused, shown on the row it was started from
 * instead of in whichever Chat is open. */
export interface ChatRowError {
  projectId: string;
  chatId: string;
  action: ChatRowAction;
  message: string;
}

/** Where a refused archive or restore reports: the row it was started from,
 * or the workbench's shared error. */
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
  /** Permanently deletes an archived Chat; callers confirm first. A refusal
   * is reported on the Chat's archived row. */
  deleteChat: (chatId: string, projectId?: string) => Promise<boolean>;
  dismissChatRowError: () => void;
}

/** The archive, restore and delete actions load with the first one used,
 * outside the initial bundle; a load failure is reported like a refusal. */
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
    action: (loaded: Awaited<ReturnType<typeof load>>) => Promise<boolean>,
  ) => {
    // An action started in one Project is dropped if the Project switches
    // while the actions load: it would run against the next Project.
    const projectId = get().project?.id;
    return load()
      .then((loaded) => get().project?.id === projectId && action(loaded))
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
      run((loaded) => loaded.archiveChat(chatId, projectId, report)),
    restoreChat: (chatId, projectId, report) =>
      run((loaded) => loaded.restoreChat(chatId, projectId, report)),
    deleteChat: (chatId, projectId) =>
      run((loaded) => loaded.deleteChat(chatId, projectId)),
  };
};

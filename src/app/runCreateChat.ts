import type { StoreApi } from "zustand";

import type {
  BootstrapSnapshot,
  ProjectLayoutRequest,
} from "../domain/workbench";
import type { ChatPort } from "../ports/workbenchPorts";
import {
  commitIdentityChatResult,
  type ProjectionOutcome,
} from "./chatTransitions";
import {
  nextProjectGeneration,
  validateTransitionIntent,
} from "./projectIntent";
import { currentProjectLayout } from "./projectProjection";
import type { WorkbenchState } from "./store";

export type CreateChatResult =
  | { status: "confirmed"; chatId: string }
  | { status: "confirmed-in-other-project" }
  | { status: "not-created" }
  | { status: "unconfirmed"; reconcile?: () => Promise<CreateChatResult> };

export async function runCreateChat(
  input: {
    set: StoreApi<WorkbenchState>["setState"];
    get: StoreApi<WorkbenchState>["getState"];
    applyProjection: (
      snapshot: BootstrapSnapshot,
      generation: number,
    ) => Promise<void>;
    loadAuthoritativeProjection: (
      generation: number,
      classify: (snapshot: BootstrapSnapshot) => ProjectionOutcome | null,
    ) => Promise<ProjectionOutcome>;
    chat: ChatPort;
  },
  projectId?: string | null,
  chatId?: string,
): Promise<CreateChatResult> {
  const { set, get, applyProjection, loadAuthoritativeProjection, chat } =
    input;
  if (get().projectTransitioning) return { status: "not-created" };
  const initial = currentProjectLayout(get());
  const sourceProjectId =
    projectId === null || (projectId === undefined && get().project?.isInternal)
      ? null
      : (projectId ?? initial?.projectId);
  const foreign =
    typeof sourceProjectId === "string" &&
    sourceProjectId !== initial?.projectId;
  if (!initial && projectId !== null) {
    if (foreign) return { status: "not-created" };
    await get().ensureLocalProject();
    return { status: "not-created" };
  }

  set({ projectTransitioning: true, error: null });
  const expectedTransitionEpoch = get().nativeTransitionEpoch;
  try {
    await get().flushActiveEditor();
  } catch {
    set({
      projectTransitioning: false,
      error: "Could not save the editor. Try again.",
    });
    return { status: "not-created" };
  }
  const current = currentProjectLayout(get());
  if (!current && projectId !== null) {
    set({
      projectTransitioning: false,
      error: "Open a project before starting a new chat.",
    });
    return { status: "not-created" };
  }
  if (foreign && current && !chatId) {
    const confirmed = await commitIdentityChatResult({
      set,
      get,
      applyProjection,
      loadAuthoritativeProjection,
      failedMessage: "Could not create the chat. Try again.",
      failureMessage:
        "Chat creation is unconfirmed. Reopen the project before retrying.",
      invoke: () =>
        chat.createChat(current, expectedTransitionEpoch, sourceProjectId),
    });
    return confirmed
      ? { status: "confirmed-in-other-project" }
      : { status: "unconfirmed" };
  }
  return createOnActiveProject({
    set,
    get,
    applyProjection,
    loadAuthoritativeProjection,
    chat,
    current,
    expectedTransitionEpoch,
    chatId: chatId ?? crypto.randomUUID(),
    targetProjectId: sourceProjectId ?? null,
  });
}

async function createOnActiveProject(input: {
  set: StoreApi<WorkbenchState>["setState"];
  get: StoreApi<WorkbenchState>["getState"];
  applyProjection: (
    snapshot: BootstrapSnapshot,
    generation: number,
  ) => Promise<void>;
  loadAuthoritativeProjection: (
    generation: number,
    classify: (snapshot: BootstrapSnapshot) => ProjectionOutcome | null,
  ) => Promise<ProjectionOutcome>;
  chat: ChatPort;
  current: ProjectLayoutRequest | null;
  targetProjectId: string | null;
  expectedTransitionEpoch: number;
  chatId: string;
}): Promise<CreateChatResult> {
  const { set, get, applyProjection, loadAuthoritativeProjection, chat } =
    input;
  // Native persists this identity as the Chat primary key in the creation
  // transaction. Recovery must never infer ownership from resource layout.
  const { chatId } = input;
  let committed = false;
  let mutationInvoked = false;
  const classify = (snapshot: BootstrapSnapshot) => {
    const archived = snapshot.archivedChats.some((chat) => chat.id === chatId);
    if (
      snapshot.transitionEpoch === input.expectedTransitionEpoch &&
      snapshot.project?.id === input.current?.projectId &&
      !snapshot.tabs.some((tab) => tab.id === chatId) &&
      !archived
    )
      return "unchanged" as const;
    if (
      input.targetProjectId
        ? snapshot.project?.id !== input.targetProjectId
        : !snapshot.project?.isInternal
    )
      return null;
    if (
      snapshot.tabs.some((tab) => tab.kind === "chat" && tab.id === chatId) ||
      archived
    ) {
      return "committed" as const;
    }
    if (!committed) {
      return "unchanged" as const;
    }
    return null;
  };
  const confirm = async (): Promise<CreateChatResult> => {
    // A later layout edit may have detached the Chat before recovery. Its
    // durable identity still belongs to this creation; reattach that Chat.
    if (get().archivedChats.some((chat) => chat.id === chatId)) {
      if (!(await get().restoreChat(chatId)))
        return { status: "unconfirmed", reconcile };
    }
    return { status: "confirmed", chatId };
  };
  const reconcile = async (): Promise<CreateChatResult> => {
    const generation = nextProjectGeneration(get().projectGeneration);
    set({
      projectGeneration: generation,
      projectTransitioning: true,
      repositoryRefreshing: false,
    });
    try {
      const outcome = await loadAuthoritativeProjection(generation, classify);
      if (outcome === "committed") return await confirm();
      if (outcome === "unchanged") {
        set({ error: "Could not create the chat. Try again." });
        return { status: "not-created" };
      }
    } catch {
      set({
        projectTransitioning: false,
        repositoryRefreshing: false,
        error:
          "Chat creation is unconfirmed. Reload chats before trying again.",
      });
    }
    return { status: "unconfirmed", reconcile };
  };
  try {
    const latest = currentProjectLayout(get());
    if (!latest && input.targetProjectId)
      throw new Error("Open a project before starting a new chat.");
    mutationInvoked = true;
    const result = await chat.createChat(
      latest,
      input.expectedTransitionEpoch,
      input.targetProjectId,
      chatId,
    );
    committed = result.committed;
    validateTransitionIntent(result, input.expectedTransitionEpoch);
    if (!result.committed) {
      set({
        projectTransitioning: false,
        error: "Could not create the chat. Try again.",
      });
      return { status: "not-created" };
    }
    const generation = nextProjectGeneration(get().projectGeneration);
    set({ projectGeneration: generation, repositoryRefreshing: false });
    if (result.snapshot) {
      if (result.snapshot.transitionEpoch !== result.transitionEpoch) {
        throw new Error(
          "native Chat creation returned inconsistent intent metadata",
        );
      }
      if (classify(result.snapshot) !== "committed") {
        throw new Error("native Chat creation returned an invalid projection");
      }
      await applyProjection(result.snapshot, generation);
    } else {
      if (
        (await loadAuthoritativeProjection(generation, classify)) !==
        "committed"
      ) {
        throw new Error("Chat creation has not been confirmed");
      }
    }
    return await confirm();
  } catch {
    if (mutationInvoked) {
      return reconcile();
    } else {
      set({
        projectTransitioning: false,
        error: "Could not create the chat. Try again.",
      });
    }
  }
  return { status: "not-created" };
}

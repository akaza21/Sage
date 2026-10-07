import type { StoreApi } from "zustand";

import {
  attachedContextKey,
  MAX_ATTACHED_CONTEXTS_PER_TURN,
  type AttachedContext,
  parseAttachedContext,
  validateAttachedContexts,
} from "../domain/attachedContext";
import {
  boundedChatError,
  MAX_QUEUED_CHAT_PROMPTS,
  validateChatComposerText,
  type ChatOwnershipIntent,
  type QueuedChatPrompt,
} from "../domain/chatOwnership";
import { errorMessage } from "../domain/errors";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import type { WorkbenchState } from "./store";
import { emptyChatOwnershipEntry as emptyEntry } from "./chatOwnershipProjection";

function normalizeEntry(
  entry: WorkbenchState["chatUiById"][string] | undefined,
) {
  return entry
    ? { ...entry, attachedContext: entry.attachedContext ?? [] }
    : emptyEntry();
}

interface Dependencies {
  get: StoreApi<WorkbenchState>["getState"];
  set: StoreApi<WorkbenchState>["setState"];
  ownsChat: (state: WorkbenchState, intent: ChatOwnershipIntent) => boolean;
  activeChatId: (state: WorkbenchState) => string | null;
}

export function createAttachedContextActions({
  get,
  set,
  ownsChat,
  activeChatId,
}: Dependencies) {
  return {
    attachChatContext(intent: ChatOwnershipIntent, context: AttachedContext) {
      const state = get();
      if (!ownsChat(state, intent)) return false;
      const entry = normalizeEntry(state.chatUiById[intent.chatId]);
      if (entry.sending) return false;
      try {
        if (
          entry.attachedContext.some(
            (candidate) =>
              attachedContextKey(candidate) === attachedContextKey(context),
          )
        ) {
          return true;
        }
        const attachedContext = validateAttachedContexts([
          ...entry.attachedContext,
          context,
        ]);
        set({
          chatUiById: {
            ...state.chatUiById,
            [intent.chatId]: { ...entry, attachedContext },
          },
        });
        return true;
      } catch (error) {
        const message = boundedChatError(errorMessage(error));
        set({
          chatUiById: {
            ...state.chatUiById,
            [intent.chatId]: { ...entry, error: message },
          },
          ...(activeChatId(state) === intent.chatId
            ? { chatError: message }
            : {}),
        });
        return false;
      }
    },
    detachChatContext(intent: ChatOwnershipIntent, contextKey: string) {
      const state = get();
      if (!ownsChat(state, intent)) return false;
      const entry = normalizeEntry(state.chatUiById[intent.chatId]);
      if (entry.sending) return false;
      const attachedContext = entry.attachedContext.filter(
        (context) => attachedContextKey(context) !== contextKey,
      );
      if (attachedContext.length === entry.attachedContext.length) return false;
      set({
        chatUiById: {
          ...state.chatUiById,
          [intent.chatId]: { ...entry, attachedContext },
        },
      });
      return true;
    },
    clearAttachedComposer(
      intent: ChatOwnershipIntent,
      expectedPrompt: string,
      expectedContext: readonly AttachedContext[],
    ) {
      const state = get();
      if (!ownsChat(state, intent)) return false;
      const entry = normalizeEntry(state.chatUiById[intent.chatId]);
      if (entry.composerText.trim() !== expectedPrompt) return false;
      const sentKeys = new Set(expectedContext.map(attachedContextKey));
      const attachedContext = entry.attachedContext.filter(
        (context) => !sentKeys.has(attachedContextKey(context)),
      );
      const updated = { ...entry, composerText: "", attachedContext };
      const active = activeChatId(state) === intent.chatId;
      set({
        chatUiById: { ...state.chatUiById, [intent.chatId]: updated },
        ...(active ? { composerText: "" } : {}),
      });
      return true;
    },
    restoreAttachedComposer(
      intent: ChatOwnershipIntent,
      composerText: string,
      context: readonly AttachedContext[],
    ) {
      const state = get();
      if (!ownsChat(state, intent)) return false;
      const entry = normalizeEntry(state.chatUiById[intent.chatId]);
      const attachedContext: AttachedContext[] = [];
      const existing = new Set<string>();
      for (const source of [entry.attachedContext, context]) {
        if (!Array.isArray(source)) continue;
        for (const candidate of source.slice(
          0,
          MAX_ATTACHED_CONTEXTS_PER_TURN,
        )) {
          if (attachedContext.length === MAX_ATTACHED_CONTEXTS_PER_TURN) break;
          const parsed = parseAttachedContext(candidate);
          if (!parsed) continue;
          const key = attachedContextKey(parsed);
          if (existing.has(key)) continue;
          existing.add(key);
          attachedContext.push(parsed);
        }
      }
      composerText = entry.composerText || composerText;
      const updated = { ...entry, composerText, attachedContext };
      const active = activeChatId(state) === intent.chatId;
      set({
        chatUiById: { ...state.chatUiById, [intent.chatId]: updated },
        ...(active ? { composerText } : {}),
      });
      return true;
    },
  };
}

const queueMutations = new Set<string>();

export async function persistQueuedChatPrompt(
  store: StoreApi<WorkbenchState>,
  chatId: string | undefined,
  bindingId: string,
  modelId: string | null,
): Promise<boolean> {
  const state = store.getState();
  const ownerId = chatId ?? state.activeTabId;
  const intent = ownerId ? state.captureChatOwnership(ownerId) : null;
  if (!intent) return false;
  const entry = state.chatUiById[intent.chatId];
  const text = (entry?.composerText ?? state.composerText).trim();
  const context = entry?.attachedContext ?? [];
  const queue = entry?.queuedPrompts ?? [];
  if (!text || queue.length >= MAX_QUEUED_CHAT_PROMPTS) return false;
  validateChatComposerText(text);
  const ownerKey = `${intent.projectId}:${intent.chatId}:${intent.ownershipEpoch}`;
  if (queueMutations.has(ownerKey)) return false;
  queueMutations.add(ownerKey);
  const prompt: QueuedChatPrompt = {
    id: crypto.randomUUID(),
    text,
    attachedContext: context,
    bindingId,
    modelId,
    status: "saving",
  };
  store.setState((current) => {
    if (
      current.project?.id !== intent.projectId ||
      current.chatOwnershipEpoch !== intent.ownershipEpoch ||
      !current.chatUiById[intent.chatId]
    )
      return current;
    const owned = current.chatUiById[intent.chatId];
    return {
      chatUiById: {
        ...current.chatUiById,
        [intent.chatId]: {
          ...owned,
          queuedPrompts: [...(owned.queuedPrompts ?? []), prompt],
        },
      },
    };
  });
  try {
    await getWorkbenchPorts().project.replaceChatQueuedPrompts(
      intent.projectId,
      intent.chatId,
      [...queue, { ...prompt, status: "queued" }],
    );
    store.getState().clearChatComposer(intent, text, context);
    store.setState((latest) => {
      if (
        latest.project?.id !== intent.projectId ||
        latest.chatOwnershipEpoch !== intent.ownershipEpoch
      )
        return latest;
      const owned = latest.chatUiById[intent.chatId];
      if (!owned) return latest;
      return {
        chatUiById: {
          ...latest.chatUiById,
          [intent.chatId]: {
            ...owned,
            queuedPrompts: (owned.queuedPrompts ?? []).map((item) =>
              item.id === prompt.id ? { ...item, status: "queued" } : item,
            ),
          },
        },
      };
    });
    return true;
  } catch {
    store.setState((latest) => {
      if (
        latest.project?.id !== intent.projectId ||
        latest.chatOwnershipEpoch !== intent.ownershipEpoch
      )
        return latest;
      const owned = latest.chatUiById[intent.chatId];
      if (!owned) return latest;
      return {
        chatUiById: {
          ...latest.chatUiById,
          [intent.chatId]: {
            ...owned,
            queuedPrompts: (owned.queuedPrompts ?? []).filter(
              (item) => item.id !== prompt.id,
            ),
          },
        },
      };
    });
    const current = store.getState();
    current.setChatActionState(intent, {
      sending: current.chatUiById[intent.chatId]?.sending ?? false,
      error: "Could not save queued message.",
    });
    return false;
  } finally {
    queueMutations.delete(ownerKey);
  }
}

export function changeQueuedChatPrompt(
  store: StoreApi<WorkbenchState>,
  chatId: string | undefined,
  id: string,
  status: QueuedChatPrompt["status"] | "deleted",
): void {
  const state = store.getState();
  const ownerId = chatId ?? state.activeTabId;
  const intent = ownerId ? state.captureChatOwnership(ownerId) : null;
  if (!intent) return;
  const setStatus = (next: QueuedChatPrompt["status"]) => {
    store.setState((current) => {
      if (
        current.project?.id !== intent.projectId ||
        current.chatOwnershipEpoch !== intent.ownershipEpoch
      )
        return current;
      const owned = current.chatUiById[intent.chatId];
      if (!owned) return current;
      return {
        chatUiById: {
          ...current.chatUiById,
          [intent.chatId]: {
            ...owned,
            queuedPrompts: (owned.queuedPrompts ?? []).map((prompt) =>
              prompt.id === id ? { ...prompt, status: next } : prompt,
            ),
          },
        },
      };
    });
  };
  if (status !== "deleted") {
    if (status === "outcomeUnknown") {
      const ownerKey = `${intent.projectId}:${intent.chatId}:${intent.ownershipEpoch}`;
      const queue = state.chatUiById[intent.chatId]?.queuedPrompts ?? [];
      if (
        queueMutations.has(ownerKey) ||
        !queue.some((prompt) => prompt.id === id)
      )
        return;
      queueMutations.add(ownerKey);
      setStatus("saving");
      void getWorkbenchPorts()
        .project.replaceChatQueuedPrompts(
          intent.projectId,
          intent.chatId,
          queue.map((prompt) =>
            prompt.id === id ? { ...prompt, status } : prompt,
          ),
        )
        .then(() => setStatus(status))
        .catch(() => {
          setStatus(status);
          const current = store.getState();
          current.setChatActionState(intent, {
            sending: current.chatUiById[intent.chatId]?.sending ?? false,
            error: "Could not save the uncertain Steer state.",
          });
        })
        .finally(() => queueMutations.delete(ownerKey));
      return;
    }
    setStatus(status);
    return;
  }
  const ownerKey = `${intent.projectId}:${intent.chatId}:${intent.ownershipEpoch}`;
  if (queueMutations.has(ownerKey)) return;
  const queue = state.chatUiById[intent.chatId]?.queuedPrompts ?? [];
  if (!queue.some((prompt) => prompt.id === id)) return;
  queueMutations.add(ownerKey);
  store.setState((current) => {
    const owned = current.chatUiById[intent.chatId];
    if (!owned) return current;
    return {
      chatUiById: {
        ...current.chatUiById,
        [intent.chatId]: {
          ...owned,
          queuedPrompts: (owned.queuedPrompts ?? []).map((prompt) =>
            prompt.id === id ? { ...prompt, status: "deleting" } : prompt,
          ),
        },
      },
    };
  });
  void getWorkbenchPorts()
    .project.replaceChatQueuedPrompts(
      intent.projectId,
      intent.chatId,
      queue.filter((prompt) => prompt.id !== id),
    )
    .then(() => {
      store.setState((current) => {
        if (
          current.project?.id !== intent.projectId ||
          current.chatOwnershipEpoch !== intent.ownershipEpoch
        )
          return current;
        const owned = current.chatUiById[intent.chatId];
        if (!owned) return current;
        return {
          chatUiById: {
            ...current.chatUiById,
            [intent.chatId]: {
              ...owned,
              queuedPrompts: (owned.queuedPrompts ?? []).filter(
                (prompt) => prompt.id !== id,
              ),
            },
          },
        };
      });
    })
    .catch(() => {
      store.setState((current) => {
        if (
          current.project?.id !== intent.projectId ||
          current.chatOwnershipEpoch !== intent.ownershipEpoch
        )
          return current;
        const owned = current.chatUiById[intent.chatId];
        if (!owned) return current;
        return {
          chatUiById: {
            ...current.chatUiById,
            [intent.chatId]: {
              ...owned,
              queuedPrompts: (owned.queuedPrompts ?? []).map((prompt) =>
                prompt.id === id ? { ...prompt, status: "blocked" } : prompt,
              ),
            },
          },
        };
      });
      const current = store.getState();
      current.setChatActionState(intent, {
        sending: current.chatUiById[intent.chatId]?.sending ?? false,
        error: "Could not remove queued message.",
      });
    })
    .finally(() => queueMutations.delete(ownerKey));
}

import type { StateCreator } from "zustand";

import type { AttachedContext } from "../domain/attachedContext";
import type { AcpCommandFailure } from "../domain/acp";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import {
  boundedChatError,
  validateChatComposerText,
  type ChatOwnershipEntry,
  type ChatOwnershipIntent,
} from "../domain/chatOwnership";
import { createAttachedContextActions } from "./chatAttachedContextState";
import {
  createChatChoiceActions,
  type ChatChoiceActions,
} from "./chatChoiceActions";
import type { WorkbenchState } from "./store";
import { nextProjectGeneration } from "./projectIntent";
import {
  editedChatEntry as edited,
  emptyChatOwnershipEntry as emptyEntry,
} from "./chatOwnershipProjection";

export interface ChatOwnershipSlice extends ChatChoiceActions {
  chatOwnershipEpoch: number;
  chatUiById: Record<string, ChatOwnershipEntry>;
  chatError: string | null;
  captureChatOwnership: (chatId?: string) => ChatOwnershipIntent | null;
  setChatComposer: (intent: ChatOwnershipIntent, text: string) => boolean;
  attachChatContext: (
    intent: ChatOwnershipIntent,
    context: AttachedContext,
  ) => boolean;
  detachChatContext: (
    intent: ChatOwnershipIntent,
    contextKey: string,
  ) => boolean;
  clearChatComposer: (
    intent: ChatOwnershipIntent,
    expectedPrompt: string,
    expectedContext: readonly AttachedContext[],
  ) => boolean;
  restoreChatComposer: (
    intent: ChatOwnershipIntent,
    prompt: string,
    context: readonly AttachedContext[],
  ) => boolean;
  setChatActionState: (
    intent: ChatOwnershipIntent,
    state: {
      sending: boolean;
      error: string | null;
      errorKind?: AcpCommandFailure["kind"] | null;
      modeError?: string | null;
      modeErrorOwner?: { sessionId: string; runtimeEpoch: string } | null;
      modeChanging?: boolean;
    },
  ) => boolean;
  clearActiveChatError: () => void;
  setComposerText: (text: string) => void;
  sendPrompt: () => Promise<void>;
}

function activeChatId(state: WorkbenchState) {
  return (
    state.tabs.find(
      (tab) => tab.id === state.activeTabId && tab.kind === "chat",
    )?.id ?? null
  );
}

function ownsChat(state: WorkbenchState, intent: ChatOwnershipIntent) {
  // Chat activation advances projection/native generations, but it must not
  // retire background work owned by the same Project lifetime. The dedicated
  // ownership epoch changes only when that lifetime is authoritatively reset.
  return (
    state.project?.id === intent.projectId &&
    state.chatOwnershipEpoch === intent.ownershipEpoch &&
    (state.tabs.some(
      (tab) => tab.id === intent.chatId && tab.kind === "chat",
    ) ||
      state.archivedChats.some((chat) => chat.id === intent.chatId))
  );
}

export const createChatOwnershipSlice: StateCreator<
  WorkbenchState,
  [],
  [],
  ChatOwnershipSlice
> = (set, get) => ({
  chatOwnershipEpoch: 0,
  chatUiById: {},
  chatError: null,
  captureChatOwnership: (requestedChatId) => {
    const state = get();
    const chatId = requestedChatId ?? activeChatId(state);
    if (
      !chatId ||
      !state.project ||
      !ownsChat(state, {
        projectId: state.project.id,
        ownershipEpoch: state.chatOwnershipEpoch,
        chatId,
        projectGeneration: state.projectGeneration,
        transitionEpoch: state.nativeTransitionEpoch,
      })
    ) {
      return null;
    }
    return {
      projectId: state.project.id,
      ownershipEpoch: state.chatOwnershipEpoch,
      chatId,
      projectGeneration: state.projectGeneration,
      transitionEpoch: state.nativeTransitionEpoch,
    };
  },
  ...createChatChoiceActions(set, get, ownsChat),
  setChatComposer: (intent, text) => {
    const state = get();
    if (!ownsChat(state, intent)) return false;
    let composerText: string;
    try {
      composerText = validateChatComposerText(text);
    } catch (error) {
      const message = boundedChatError(errorMessage(error));
      const entry = state.chatUiById[intent.chatId] ?? emptyEntry();
      const active = activeChatId(state) === intent.chatId;
      set({
        chatUiById: {
          ...state.chatUiById,
          [intent.chatId]: { ...entry, error: message },
        },
        ...(active ? { chatError: message } : {}),
      });
      return false;
    }
    const entry = state.chatUiById[intent.chatId] ?? emptyEntry();
    const updated = { ...edited(entry), composerText };
    const active = activeChatId(state) === intent.chatId;
    set({
      chatUiById: { ...state.chatUiById, [intent.chatId]: updated },
      ...(active ? { composerText } : {}),
    });
    return true;
  },
  attachChatContext: (intent, context) =>
    createAttachedContextActions({
      get,
      set,
      ownsChat,
      activeChatId,
    }).attachChatContext(intent, context),
  detachChatContext: (intent, contextKey) =>
    createAttachedContextActions({
      get,
      set,
      ownsChat,
      activeChatId,
    }).detachChatContext(intent, contextKey),
  clearChatComposer: (intent, prompt, context) =>
    createAttachedContextActions({
      get,
      set,
      ownsChat,
      activeChatId,
    }).clearAttachedComposer(intent, prompt, context),
  restoreChatComposer: (intent, prompt, context) => {
    try {
      return createAttachedContextActions({
        get,
        set,
        ownsChat,
        activeChatId,
      }).restoreAttachedComposer(
        intent,
        validateChatComposerText(prompt),
        context,
      );
    } catch {
      return false;
    }
  },
  setChatActionState: (intent, actionState) => {
    const state = get();
    if (!ownsChat(state, intent)) return false;
    const entry = state.chatUiById[intent.chatId] ?? emptyEntry();
    const updated = {
      ...entry,
      sending: actionState.sending,
      error:
        actionState.error === null ? null : boundedChatError(actionState.error),
      errorKind:
        actionState.errorKind === undefined
          ? entry.errorKind
          : actionState.errorKind,
      modeError:
        actionState.modeError === undefined
          ? entry.modeError
          : actionState.modeError === null
            ? null
            : boundedChatError(actionState.modeError),
      modeErrorOwner:
        actionState.modeErrorOwner === undefined
          ? entry.modeErrorOwner
          : actionState.modeErrorOwner,
      modeChanging: actionState.modeChanging ?? entry.modeChanging ?? false,
    };
    const active = activeChatId(state) === intent.chatId;
    set({
      chatUiById: { ...state.chatUiById, [intent.chatId]: updated },
      ...(active ? { sending: updated.sending, chatError: updated.error } : {}),
    });
    return true;
  },
  clearActiveChatError: () => {
    const state = get();
    const chatId = activeChatId(state);
    if (!chatId) return;
    const entry = state.chatUiById[chatId] ?? emptyEntry();
    set({
      chatError: null,
      chatUiById: {
        ...state.chatUiById,
        [chatId]: { ...entry, error: null },
      },
    });
  },
  setComposerText: (text) => {
    try {
      const composerText = validateChatComposerText(text);
      const state = get();
      const active = state.tabs.find(
        (tab) => tab.id === state.activeTabId && tab.kind === "chat",
      );
      if (!active) return;
      set({
        composerText,
        chatUiById: {
          ...state.chatUiById,
          [active.id]: {
            ...edited(state.chatUiById[active.id] ?? emptyEntry()),
            composerText,
          },
        },
      });
    } catch (error) {
      const state = get();
      const active = state.tabs.find(
        (tab) => tab.id === state.activeTabId && tab.kind === "chat",
      );
      if (!active) return;
      const message = boundedChatError(errorMessage(error));
      set({
        chatError: message,
        chatUiById: {
          ...state.chatUiById,
          [active.id]: {
            ...(state.chatUiById[active.id] ?? emptyEntry()),
            composerText: state.composerText,
            error: message,
          },
        },
      });
    }
  },
  sendPrompt: async () => {
    const state = get();
    const chatId = activeChatId(state);
    const text = state.composerText.trim();
    const chatSelection = chatId ? state.chatUiById[chatId] : null;
    const selectedHarnessId = chatId
      ? (chatSelection?.harnessId ?? state.selectedHarnessId)
      : null;
    const selectedModelId = chatId
      ? chatSelection?.harnessId
        ? (chatSelection.modelId ?? null)
        : state.selectedModelId
      : null;
    if (
      !text ||
      !chatId ||
      !state.project ||
      !selectedHarnessId ||
      state.sending ||
      state.projectTransitioning
    ) {
      return;
    }
    const currentEntry = {
      ...(state.chatUiById[chatId] ?? emptyEntry()),
      composerText: state.composerText,
      sending: state.sending,
      error: state.chatError,
    };
    const requestGeneration = nextProjectGeneration(
      currentEntry.requestGeneration,
    );
    const owner: ChatOwnershipIntent = {
      projectId: state.project.id,
      ownershipEpoch: state.chatOwnershipEpoch,
      chatId,
      projectGeneration: state.projectGeneration,
      transitionEpoch: state.nativeTransitionEpoch,
    };
    const pendingEntry: ChatOwnershipEntry = {
      ...currentEntry,
      composerText: "",
      sending: true,
      error: null,
      requestGeneration,
    };
    set({
      chatUiById: { ...state.chatUiById, [chatId]: pendingEntry },
      composerText: "",
      sending: true,
      chatError: null,
      sendRequestGeneration: requestGeneration,
    });
    try {
      const result = await getWorkbenchPorts().agent.sendPrompt({
        chatId,
        harnessId: selectedHarnessId,
        modelId: selectedModelId,
        text,
      });
      const current = get();
      const owned = current.chatUiById[chatId];
      if (
        !ownsChat(current, owner) ||
        owned?.requestGeneration !== requestGeneration
      ) {
        return;
      }
      const completed = { ...owned, sending: false };
      const active = activeChatId(current) === chatId;
      set({
        chatUiById: { ...current.chatUiById, [chatId]: completed },
        ...(active
          ? {
              sessionId: result.sessionId,
              messages: result.messages,
              composerText: completed.composerText,
              sending: false,
              chatError: completed.error,
            }
          : activeChatId(current) === null
            ? { sending: false, chatError: null }
            : {}),
      });
    } catch (error) {
      const current = get();
      const owned = current.chatUiById[chatId];
      if (
        !ownsChat(current, owner) ||
        owned?.requestGeneration !== requestGeneration
      ) {
        return;
      }
      const failed = {
        ...owned,
        composerText: owned.composerText || text,
        sending: false,
        error: boundedChatError(errorMessage(error)),
      };
      const active = activeChatId(current) === chatId;
      set({
        chatUiById: { ...current.chatUiById, [chatId]: failed },
        ...(active
          ? {
              composerText: failed.composerText,
              sending: false,
              chatError: failed.error,
            }
          : activeChatId(current) === null
            ? { sending: false, chatError: null }
            : {}),
      });
    }
  },
});
import { errorMessage } from "../domain/errors";

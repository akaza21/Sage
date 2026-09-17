import type { StoreApi } from "zustand";

import {
  attachedContextKey,
  MAX_ATTACHED_CONTEXTS_PER_TURN,
  type DiffLineCommentContext,
  parseDiffLineCommentContext,
  validateAttachedContexts,
} from "../domain/attachedContext";
import {
  boundedChatError,
  type ChatOwnershipIntent,
} from "../domain/chatOwnership";
import { errorMessage } from "../domain/errors";
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
    attachChatContext(
      intent: ChatOwnershipIntent,
      context: DiffLineCommentContext,
    ) {
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
      expectedContext: readonly DiffLineCommentContext[],
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
      context: readonly DiffLineCommentContext[],
    ) {
      const state = get();
      if (!ownsChat(state, intent)) return false;
      const entry = normalizeEntry(state.chatUiById[intent.chatId]);
      const attachedContext: DiffLineCommentContext[] = [];
      const existing = new Set<string>();
      for (const source of [entry.attachedContext, context]) {
        if (!Array.isArray(source)) continue;
        for (const candidate of source.slice(
          0,
          MAX_ATTACHED_CONTEXTS_PER_TURN,
        )) {
          if (attachedContext.length === MAX_ATTACHED_CONTEXTS_PER_TURN) break;
          const parsed = parseDiffLineCommentContext(candidate);
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

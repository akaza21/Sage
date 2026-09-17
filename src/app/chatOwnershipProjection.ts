import {
  MAX_ARCHIVED_CHAT_OWNERSHIP_ENTRIES,
  MAX_ATTACHED_CHAT_OWNERSHIP_ENTRIES,
  MAX_CHAT_OWNERSHIP_ENTRIES,
  type ChatOwnershipEntry,
} from "../domain/chatOwnership";
import type { BootstrapSnapshot } from "../domain/workbench";
import { nextProjectGeneration } from "./projectIntent";
import type { WorkbenchState } from "./store";

export function emptyChatOwnershipEntry(): ChatOwnershipEntry {
  return {
    composerText: "",
    attachedContext: [],
    sending: false,
    error: null,
    requestGeneration: 0,
  };
}

function knownChatIds(snapshot: BootstrapSnapshot) {
  const attached = snapshot.tabs
    .filter((tab) => tab.kind === "chat")
    .map((tab) => tab.id);
  const archived = snapshot.archivedChats.map((chat) => chat.id);
  if (attached.length > MAX_ATTACHED_CHAT_OWNERSHIP_ENTRIES) {
    throw new Error(
      `attached Chat ownership entries exceed ${MAX_ATTACHED_CHAT_OWNERSHIP_ENTRIES}`,
    );
  }
  if (archived.length > MAX_ARCHIVED_CHAT_OWNERSHIP_ENTRIES) {
    throw new Error(
      `archived Chat ownership entries exceed ${MAX_ARCHIVED_CHAT_OWNERSHIP_ENTRIES}`,
    );
  }
  const ids = [...attached, ...archived];
  if (new Set(ids).size !== ids.length) {
    throw new Error("Chat ownership projection contains duplicate identities");
  }
  if (ids.length > MAX_CHAT_OWNERSHIP_ENTRIES) {
    throw new Error(
      `Chat ownership entries exceed ${MAX_CHAT_OWNERSHIP_ENTRIES}`,
    );
  }
  return ids;
}

export function projectChatOwnership(
  state: WorkbenchState,
  snapshot: BootstrapSnapshot,
  preserve: boolean,
) {
  const ids = knownChatIds(snapshot);
  const retained: Record<string, ChatOwnershipEntry> = {};
  const sameProject = state.project?.id === snapshot.project?.id;
  const previous = { ...state.chatUiById };
  const source = state.tabs.find(
    (tab) => tab.id === state.activeTabId && tab.kind === "chat",
  );
  if (preserve && sameProject && source) {
    previous[source.id] = {
      ...(previous[source.id] ?? emptyChatOwnershipEntry()),
      composerText: state.composerText,
      sending: state.sending,
      error: state.chatError,
    };
  }
  for (const id of ids) {
    retained[id] =
      preserve && sameProject
        ? (previous[id] ?? emptyChatOwnershipEntry())
        : emptyChatOwnershipEntry();
  }
  const active = snapshot.tabs.find(
    (tab) => tab.id === snapshot.activeTabId && tab.kind === "chat",
  );
  return {
    chatOwnershipEpoch:
      preserve && sameProject
        ? state.chatOwnershipEpoch
        : nextProjectGeneration(state.chatOwnershipEpoch),
    chatUiById: retained,
    composerText: active ? (retained[active.id]?.composerText ?? "") : "",
    sending: active ? (retained[active.id]?.sending ?? false) : false,
    chatError: active ? (retained[active.id]?.error ?? null) : null,
  };
}

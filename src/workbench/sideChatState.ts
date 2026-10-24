import { create } from "zustand";

/** A temporary side chat; Rust owns the registry, this is its WebView mirror. */
export interface SideChatEntry {
  id: string;
  projectId: string;
  parentChatId: string;
  running?: boolean;
}

/** At most one side chat per Project and eight in all (Rust enforces the same). */
export const MAX_SIDE_CHATS = 8;
/** Codex opens a side chat at about 43 / 57; the split is never persisted. */
export const SIDE_CHAT_SPLIT = 43;

export const useSideChatState = create<{
  sides: readonly SideChatEntry[];
  pendingClose: string | null;
  split: number;
}>(() => ({ sides: [], pendingClose: null, split: SIDE_CHAT_SPLIT }));

/** Rust (and the visual fixture) mint temporary Chat ids with this prefix. */
export const isSideChatId = (id: string | null | undefined) =>
  Boolean(id?.startsWith("side-"));

/** A parent Chat reads as working while one of its side chats runs. */
export function useParentChatStatus() {
  const sides = useSideChatState((state) => state.sides);
  return <S extends string | undefined>(chatId: string, status: S) =>
    sides.some((side) => side.parentChatId === chatId && side.running)
      ? "working"
      : status;
}

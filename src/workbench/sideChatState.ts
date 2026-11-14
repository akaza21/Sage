import { create } from "zustand";

/** Rust owns temporary chats; this is the disposable mirror. */
export interface SideChatEntry {
  id: string;
  projectId: string;
  parentChatId: string;
  running?: boolean;
}

/** At most one side chat per Project and eight in all (Rust enforces the same). */
export const MAX_SIDE_CHATS = 8;
export const SIDE_CHAT_SPLIT = 43;

export const useSideChatState = create<{
  sides: readonly SideChatEntry[];
  pendingClose: string | null;
  split: number;
  pairedId: string | null;
}>(() => ({
  sides: [],
  pendingClose: null,
  split: SIDE_CHAT_SPLIT,
  pairedId: null,
}));

/** The "side-" prefix is shared with Rust and fixtures. */
export const isSideChatId = (id: string | null | undefined) =>
  Boolean(id?.startsWith("side-"));

export function railChatIdIn(
  sides: readonly SideChatEntry[],
  activeId: string | null,
) {
  return sides.find((side) => side.id === activeId)?.parentChatId ?? activeId;
}

export function useRailChatId(activeId: string | null) {
  return useSideChatState((state) => railChatIdIn(state.sides, activeId));
}

export function useParentChatStatus() {
  const sides = useSideChatState((state) => state.sides);
  return <S extends string | undefined>(chatId: string, status: S) =>
    sides.some((side) => side.parentChatId === chatId && side.running)
      ? "working"
      : status;
}

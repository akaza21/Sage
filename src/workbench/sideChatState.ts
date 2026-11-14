import { create } from "zustand";
import { draftShownFor } from "../app/newChatDraftSlice";
import { useWorkbenchStore } from "../app/store";

/** Rust owns temporary chats; this is the disposable mirror. */
export interface SideChatEntry {
  id: string;
  projectId: string;
  parentChatId: string;
  running?: boolean;
}

/** At most one side chat per Project and eight in all (Rust enforces the same). */
export const MAX_SIDE_CHATS = 8;
/** Non-persisted 43/57 split. */
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

/** Rust/fixture temporary Chat ID prefix. */
export const isSideChatId = (id: string | null | undefined) =>
  Boolean(id?.startsWith("side-"));

/** The rail chat for `activeId`: a side chat's parent, else itself. */
export function railChatIdIn(
  sides: readonly SideChatEntry[],
  activeId: string | null,
) {
  return sides.find((side) => side.id === activeId)?.parentChatId ?? activeId;
}

/** A focused side chat isn't in the rail; its parent retains window title/highlight. */
export function useRailChatId(activeId: string | null) {
  const id = useSideChatState((state) => railChatIdIn(state.sides, activeId));
  return useWorkbenchStore((state) => draftShownFor(state, id)) ? null : id;
}

/** A parent Chat reads as working while one of its side chats runs. */
export function useParentChatStatus() {
  const sides = useSideChatState((state) => state.sides);
  return <S extends string | undefined>(chatId: string, status: S) =>
    sides.some((side) => side.parentChatId === chatId && side.running)
      ? "working"
      : status;
}

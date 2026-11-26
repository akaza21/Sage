import { useEffect, useRef, useState, useSyncExternalStore } from "react";
/** What a chat's latest turn waits on the user for. */
export type RailAttention = "approval" | "input";

interface ArchiveUndo {
  projectId: string;
  chatId: string;
  title: string;
}
const MAX_INTERACTION_CHATS = 64;
export const ARCHIVE_UNDO_MS = 8_000;
let undoTimer: ReturnType<typeof setTimeout> | undefined;
const listeners = new Set<() => void>();
let state: {
  unread: ReadonlySet<string>;
  attention: ReadonlyMap<string, RailAttention>;
  undo: ArchiveUndo | null;
} = { unread: new Set(), attention: new Map(), undo: null };
function publish() {
  for (const listener of listeners) listener();
}
export function useAgentRailInteractions() {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    () => state,
  );
}
export function markAgentRailUnread(chatId: string) {
  const unread = new Set(state.unread);
  if (unread.size >= MAX_INTERACTION_CHATS)
    unread.delete(unread.values().next().value!);
  unread.add(chatId);
  state = { ...state, unread };
  publish();
}
export function clearAgentRailUnread(chatId?: string) {
  if (chatId && !state.unread.has(chatId)) return;
  const unread = new Set(state.unread);
  if (chatId) unread.delete(chatId);
  else unread.clear();
  state = { ...state, unread };
  publish();
}
export function setAgentRailArchiveUndo(
  undo: ArchiveUndo | null,
  expected?: ArchiveUndo,
) {
  if (expected && state.undo !== expected) return;
  if (undoTimer) clearTimeout(undoTimer);
  undoTimer = undo
    ? setTimeout(() => setAgentRailArchiveUndo(null, undo), ARCHIVE_UNDO_MS)
    : undefined;
  state = { ...state, undo };
  publish();
}
/** What a chat's latest turn waits on the user for, kept while it unmounts. */
export function setAgentRailAttention(chatId: string, kind?: RailAttention) {
  if (state.attention.get(chatId) === kind) return;
  const attention = new Map(state.attention);
  if (!kind) attention.delete(chatId);
  else {
    if (attention.size >= MAX_INTERACTION_CHATS)
      attention.delete(attention.keys().next().value!);
    attention.set(chatId, kind);
  }
  state = { ...state, attention };
  publish();
}
export function resetAgentRailInteractions() {
  if (undoTimer) clearTimeout(undoTimer);
  undoTimer = undefined;
  state = { unread: new Set(), attention: new Map(), undo: null };
  publish();
}

export function pruneAgentRailInteractions(chatIds: ReadonlySet<string>) {
  const unread = new Set([...state.unread].filter((id) => chatIds.has(id)));
  const attention = new Map(
    [...state.attention].filter(([id]) => chatIds.has(id)),
  );
  if (
    unread.size === state.unread.size &&
    attention.size === state.attention.size
  )
    return;
  state = { ...state, unread, attention };
  publish();
}

const HOVER_CARD_DELAY_MS = 500;

export function useRailHoverCard() {
  const [card, setCard] = useState<{
    chatId: string;
    row: DOMRect;
    shownAt: number;
  } | null>(null);
  const timer = useRef<number | null>(null);
  const hide = () => {
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = null;
    setCard(null);
  };
  useEffect(() => {
    if (!card) return;
    window.addEventListener("scroll", hide, { capture: true, once: true });
    return () => window.removeEventListener("scroll", hide, { capture: true });
  }, [card]);
  useEffect(
    () => () => {
      if (timer.current !== null) window.clearTimeout(timer.current);
    },
    [],
  );
  const show = (chatId: string, row: HTMLElement) => {
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      timer.current = null;
      if (row.isConnected)
        setCard({
          chatId,
          row: row.getBoundingClientRect(),
          shownAt: Date.now(),
        });
    }, HOVER_CARD_DELAY_MS);
  };
  return { card, show, hide };
}

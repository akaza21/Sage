import { useEffect, useRef, useState, useSyncExternalStore } from "react";

interface ArchiveUndo {
  projectId: string;
  chatId: string;
  title: string;
}
// Interaction badges retain only the same bounded working set as the rail.
const MAX_INTERACTION_CHATS = 64;
// An archive undo is a short-lived action, not persistent navigation.
export const ARCHIVE_UNDO_MS = 8_000;
let undoTimer: ReturnType<typeof setTimeout> | undefined;
const listeners = new Set<() => void>();
let state: {
  unread: ReadonlySet<string>;
  approval: ReadonlySet<string>;
  undo: ArchiveUndo | null;
} = { unread: new Set(), approval: new Set(), undo: null };
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
export function setAgentRailApproval(chatId: string, pending: boolean) {
  if (state.approval.has(chatId) === pending) return;
  const approval = new Set(state.approval);
  if (!pending) approval.delete(chatId);
  else {
    if (approval.size >= MAX_INTERACTION_CHATS)
      approval.delete(approval.values().next().value!);
    approval.add(chatId);
  }
  state = { ...state, approval };
  publish();
}
export function resetAgentRailInteractions() {
  if (undoTimer) clearTimeout(undoTimer);
  undoTimer = undefined;
  state = { unread: new Set(), approval: new Set(), undo: null };
  publish();
}

export function pruneAgentRailInteractions(chatIds: ReadonlySet<string>) {
  const unread = new Set([...state.unread].filter((id) => chatIds.has(id)));
  const approval = new Set([...state.approval].filter((id) => chatIds.has(id)));
  if (
    unread.size === state.unread.size &&
    approval.size === state.approval.size
  )
    return;
  state = { ...state, unread, approval };
  publish();
}

/** How long the pointer rests on a rail row before its hover card shows. */
const HOVER_CARD_DELAY_MS = 500;
const MONTH_MS = 30 * 86_400_000;

/** Codex's compact age: now, then m, h, d, mo and y. */
export function agentRailHoverAge(updatedAt: number | undefined, now: number) {
  if (!updatedAt || !Number.isFinite(updatedAt) || now < updatedAt) return "";
  const minutes = Math.floor((now - updatedAt) / 60_000);
  if (minutes < 1) return "now";
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}d`;
  const months = Math.floor((now - updatedAt) / MONTH_MS);
  return months < 12 ? `${months}mo` : `${Math.floor(months / 12)}y`;
}

/**
 * Which rail row's hover card shows, after the pointer rests on it. Leaving
 * the row, pressing it or scrolling anywhere hides the card.
 */
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

import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type RefObject,
} from "react";
import type { TurnProjection } from "../../domain/acp";
import { ChatFindBar } from "./ChatFindBar";
import {
  findChatMatches,
  stepChatMatch,
  type ChatMatch,
} from "./chatFindModel";
import type { TurnListNavigation } from "./chatScroll";
import { useChatFindHighlights } from "./useChatFindHighlights";

export const CHAT_FIND_DEBOUNCE_MS = 200;
const NO_MATCHES: readonly ChatMatch[] = [];

export interface ChatFindSessionProps {
  turns: readonly TurnProjection[];
  scrollport: HTMLElement;
  navigationRef: RefObject<TurnListNavigation | null>;
  /** Earlier Turns exist that are not loaded or were discarded. */
  incomplete: boolean;
  focusRequest: number;
  onClose: () => void;
  /** The pane-held query of an open find, so a remount resumes it. */
  initialQuery?: string;
  onQueryChange?: (query: string) => void;
}

function scopeNote(
  incomplete: boolean,
  result: { scanLimited: boolean; previewLimited: boolean },
) {
  const notes: string[] = [];
  if (incomplete) notes.push("Only loaded turns are searched.");
  if (result.scanLimited) notes.push("Search stopped early in this long chat.");
  if (result.previewLimited)
    notes.push("Long messages are searched as far as they are shown.");
  return notes.length ? notes.join(" ") : null;
}

/** The open find box for one chat pane: debounced search plus highlights. */
export function ChatFindSession({
  turns,
  scrollport,
  navigationRef,
  incomplete,
  focusRequest,
  onClose,
  initialQuery = "",
  onQueryChange,
}: ChatFindSessionProps) {
  // A remounted find resumes its query already settled.
  const [query, setQueryState] = useState(initialQuery);
  const [settledQuery, setSettledQuery] = useState(initialQuery);
  const setQuery = useCallback(
    (next: string) => {
      onQueryChange?.(next);
      setQueryState(next);
    },
    [onQueryChange],
  );
  // The selected match is held by key, so streaming updates that recompute
  // the matches keep the reader's place instead of resetting to the first.
  const [selected, setSelected] = useState<{ query: string; key: string }>();
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    const timer = setTimeout(
      () => setSettledQuery(query),
      CHAT_FIND_DEBOUNCE_MS,
    );
    return () => clearTimeout(timer);
  }, [query]);
  const result = useMemo(
    () => findChatMatches(turns, settledQuery),
    [turns, settledQuery],
  );
  const pending = query !== settledQuery;
  const matches = query && !pending ? result.matches : NO_MATCHES;
  const selectedIndex =
    selected?.query === query
      ? matches.findIndex((match) => match.key === selected.key)
      : -1;
  const index = Math.max(0, selectedIndex);
  const step = useCallback(
    (direction: 1 | -1) => {
      const match = matches[stepChatMatch(index, direction, matches.length)];
      if (!match) return;
      setSelected({ query, key: match.key });
      setRevision((value) => value + 1);
    },
    [index, matches, query],
  );
  useChatFindHighlights({
    scrollport,
    navigationRef,
    matches,
    current: index,
    revision,
  });
  useEffect(() => {
    const surface = scrollport.closest(".chat-surface") ?? scrollport;
    const onKey = (event: Event) => {
      const key = event as KeyboardEvent;
      if (key.isComposing || key.ctrlKey || key.metaKey || key.altKey) return;
      const inBox =
        key.target instanceof Element && !!key.target.closest(".chat-find");
      if (key.key !== "F3" && !(key.key === "Enter" && inBox)) return;
      // Enter on a box button activates that button instead.
      if (key.key === "Enter" && key.target instanceof HTMLButtonElement)
        return;
      key.preventDefault();
      key.stopPropagation();
      step(key.shiftKey ? -1 : 1);
    };
    surface.addEventListener("keydown", onKey);
    return () => surface.removeEventListener("keydown", onKey);
  }, [scrollport, step]);
  return (
    <ChatFindBar
      query={query}
      onQuery={setQuery}
      onClose={onClose}
      onStep={step}
      index={index}
      count={matches.length}
      capped={!pending && result.capped}
      pending={pending}
      focusRequest={focusRequest}
      scopeNote={scopeNote(incomplete, result)}
    />
  );
}

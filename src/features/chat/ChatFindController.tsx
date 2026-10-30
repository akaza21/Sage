import {
  useCallback,
  useEffect,
  useState,
  type RefObject,
  useMemo,
  useLayoutEffect,
  useRef,
} from "react";
import { type TurnProjection } from "../../domain/acp";
import { keymapBinding, matchesKeymapChord } from "../../domain/keymapRegistry";
import { type TurnListNavigation } from "./chatScroll";
import {
  findChatMatches,
  stepChatMatch,
  type ChatMatch,
  chatFindStatus,
  MAX_CHAT_FIND_QUERY,
} from "./chatFindModel";
import { useChatFindHighlights } from "./useChatFindHighlights";
import {
  ArrowDown,
  ArrowUp,
  LoaderCircle,
  Search,
  X,
} from "../../ui/leanIcons";
import "./chat-find.css";

interface Props {
  turns: readonly TurnProjection[];
  scrollport: HTMLElement;
  navigationRef: RefObject<TurnListNavigation | null>;
  incomplete: boolean;
  chatId?: string;
}

// The chat pane the pointer last pressed in. With focus on <body> (after a
// click on plain transcript text) Ctrl+F goes to that pane, and to no other.
let lastPressedSurface: Element | null = null;

// An open find outlives a transcript remount in its pane: a chat activation
// reloads the authoritative projection, which unmounts the transcript. Keyed
// by the pane's surface element, so a closed pane forgets it, and by Chat,
// so a pane that now shows another Chat starts without one.
const openFinds = new WeakMap<Element, { chatId?: string; query: string }>();

function surfaceVisible(surface: Element) {
  return typeof surface.checkVisibility === "function"
    ? surface.checkVisibility()
    : surface.isConnected;
}

/** Owns Ctrl+F for one chat pane; editors keep their own Ctrl+F. */
export function ChatFindController(props: Props) {
  const { scrollport, chatId, ...sessionProps } = props;
  const pane = scrollport.closest(".chat-surface") ?? scrollport;
  const resumed = openFinds.get(pane);
  const [open, setOpen] = useState(
    () => resumed !== undefined && resumed.chatId === chatId,
  );
  const [focusRequest, setFocusRequest] = useState(0);
  useEffect(() => {
    // The pane now shows another Chat: the kept find belonged to the old one
    // and must not come back when the pane returns to it.
    const kept = openFinds.get(pane);
    if (kept && kept.chatId !== chatId) openFinds.delete(pane);
  }, [chatId, pane]);
  const remember = useCallback(
    (query: string) => {
      if (openFinds.has(pane)) openFinds.set(pane, { chatId, query });
    },
    [chatId, pane],
  );
  const close = useCallback(() => {
    openFinds.delete(scrollport.closest(".chat-surface") ?? scrollport);
    setOpen(false);
    scrollport
      .closest(".chat-surface")
      ?.querySelector<HTMLTextAreaElement>(".composer textarea")
      ?.focus({ preventScroll: true });
  }, [scrollport]);
  useEffect(() => {
    const surface = scrollport.closest(".chat-surface");
    if (!surface) return;
    const press = () => {
      lastPressedSurface = surface;
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.isComposing) return;
      const target = event.target;
      const inSurface = target instanceof Node && surface.contains(target);
      const unfocused =
        target === document.body || target === document.documentElement;
      if (
        !inSurface &&
        !(
          unfocused &&
          lastPressedSurface === surface &&
          surfaceVisible(surface)
        )
      )
        return;
      if (target instanceof Element && target.closest(".cm-editor")) return;
      if (matchesKeymapChord(keymapBinding("chat.findInChat"), event)) {
        event.preventDefault();
        event.stopPropagation();
        const kept = openFinds.get(surface);
        if (kept === undefined || kept.chatId !== chatId)
          openFinds.set(surface, { chatId, query: "" });
        setOpen(true);
        setFocusRequest((value) => value + 1);
        return;
      }
      if (
        open &&
        event.key === "Escape" &&
        !event.ctrlKey &&
        !event.metaKey &&
        !event.altKey &&
        target instanceof Element &&
        target.closest(".chat-find")
      ) {
        event.preventDefault();
        event.stopPropagation();
        close();
      }
    };
    surface.addEventListener("pointerdown", press, true);
    window.addEventListener("keydown", onKey);
    return () => {
      surface.removeEventListener("pointerdown", press, true);
      window.removeEventListener("keydown", onKey);
      if (lastPressedSurface === surface) lastPressedSurface = null;
    };
  }, [chatId, close, open, scrollport]);
  return open ? (
    <ChatFindSession
      {...sessionProps}
      scrollport={scrollport}
      initialQuery={resumed && resumed.chatId === chatId ? resumed.query : ""}
      onQueryChange={remember}
      focusRequest={focusRequest}
      onClose={close}
    />
  ) : null;
}

const CHAT_FIND_DEBOUNCE_MS = 200;

const NO_MATCHES: readonly ChatMatch[] = [];

interface ChatFindSessionProps {
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
function ChatFindSession({
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

/** Codex "Search chat…" box: one row when empty, a result row once typed. */
function ChatFindBar({
  query,
  onQuery,
  onClose,
  onStep,
  index,
  count,
  capped,
  pending,
  focusRequest,
  scopeNote,
}: {
  query: string;
  onQuery: (query: string) => void;
  onClose: () => void;
  onStep: (direction: 1 | -1) => void;
  index: number;
  count: number;
  capped: boolean;
  pending: boolean;
  /** Bumped by each Ctrl+F, so a repeat press refocuses and selects. */
  focusRequest: number;
  scopeNote: string | null;
}) {
  const input = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => {
    // 0 is a find resumed after a transcript remount: never steal focus.
    if (focusRequest === 0) return;
    input.current?.focus();
    input.current?.select();
  }, [focusRequest]);
  const Icon = pending ? LoaderCircle : Search;
  const stepDisabled = pending || !count;
  return (
    <section
      className="chat-find"
      role="search"
      aria-label="Find in chat"
      data-pending={pending || undefined}
    >
      <div className="chat-find__input">
        <Icon
          size={pending ? 14 : 10}
          aria-hidden="true"
          className={pending ? "chat-find__spinner" : "chat-find__icon"}
        />
        <input
          ref={input}
          type="search"
          aria-label="Search chat"
          placeholder="Search chat…"
          value={query}
          maxLength={MAX_CHAT_FIND_QUERY}
          spellCheck={false}
          autoComplete="off"
          onChange={(event) =>
            onQuery(event.target.value.slice(0, MAX_CHAT_FIND_QUERY))
          }
        />
        <button
          type="button"
          className="chat-find__close"
          aria-label="Close search"
          onClick={onClose}
        >
          <X size={14} aria-hidden="true" />
        </button>
      </div>
      {query ? (
        <div className="chat-find__results">
          <button
            type="button"
            aria-label="Previous match"
            disabled={stepDisabled}
            onClick={() => onStep(-1)}
          >
            <ArrowUp size={14} aria-hidden="true" />
          </button>
          <button
            type="button"
            aria-label="Next match"
            disabled={stepDisabled}
            onClick={() => onStep(1)}
          >
            <ArrowDown size={14} aria-hidden="true" />
          </button>
          <span role="status" aria-live="polite" aria-atomic="true">
            {chatFindStatus({ pending, index, count, capped })}
          </span>
        </div>
      ) : null}
      {query && scopeNote ? (
        <p className="chat-find__scope">{scopeNote}</p>
      ) : null}
    </section>
  );
}

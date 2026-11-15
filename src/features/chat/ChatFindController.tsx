import { useCallback, useEffect, useState, type RefObject } from "react";
import { type TurnProjection } from "../../domain/acp";
import { keymapBinding, matchesKeymapChord } from "../../domain/keymapRegistry";
import "./chat-find.css";
import { ChatFindSession } from "./ChatFindSession";
import { type TurnListNavigation } from "./chatScroll";

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

// A chat activation reloads the projection and unmounts the transcript, so an
// open find is kept here to survive that remount.
const openFinds = new WeakMap<Element, { chatId?: string; query: string }>();

function surfaceVisible(surface: Element) {
  return typeof surface.checkVisibility === "function"
    ? surface.checkVisibility()
    : surface.isConnected;
}

export function ChatFindController(props: Props) {
  const { scrollport, chatId, ...sessionProps } = props;
  const pane = scrollport.closest(".chat-surface") ?? scrollport;
  const resumed = openFinds.get(pane);
  const [open, setOpen] = useState(
    () => resumed !== undefined && resumed.chatId === chatId,
  );
  const [focusRequest, setFocusRequest] = useState(0);
  useEffect(() => {
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

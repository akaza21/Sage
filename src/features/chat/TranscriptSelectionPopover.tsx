import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { TranscriptQuote } from "../../domain/transcriptQuote";
import {
  readTranscriptQuote,
  selectAnswerText,
  selectionPopoverPosition,
} from "./transcriptSelection";

type Selected = NonNullable<ReturnType<typeof readTranscriptQuote>> & {
  keyboard: boolean;
};

const isSelectionMenuKey = (event: KeyboardEvent) =>
  event.key === "ContextMenu" || (event.key === "F10" && event.shiftKey);

/**
 * Codex "Add to chat | Ask in side chat" toolbar. Content listeners read the
 * selection only on pointer-up, Shift+Arrow key-up or the menu key; dismiss
 * listeners exist only while the toolbar is shown.
 */
export function TranscriptSelectionPopover({
  content,
  scrollport,
  takeInitialKeyboard,
  onAdd,
  onSideChat,
}: {
  content: HTMLElement;
  scrollport: HTMLElement;
  /** Consumed once on mount; true when the menu key armed the chunk. */
  takeInitialKeyboard?: () => boolean;
  onAdd: (quote: TranscriptQuote) => void;
  onSideChat: (quote: TranscriptQuote) => void;
}) {
  const [selected, setSelected] = useState<Selected | null>(null);
  const [position, setPosition] = useState({ left: 0, top: 0 });
  const toolbar = useRef<HTMLDivElement>(null);
  const returnFocus = useRef<HTMLElement | null>(null);
  useEffect(() => {
    const show = (keyboard: boolean) => {
      const active = document.activeElement;
      const value =
        readTranscriptQuote(window.getSelection(), content) ??
        (keyboard && active instanceof HTMLElement
          ? selectAnswerText(active, content)
          : null);
      if (!value) return setSelected(null);
      returnFocus.current =
        active instanceof HTMLElement && active !== document.body
          ? active
          : value.answer;
      setSelected({ ...value, keyboard });
    };
    const pointerUp = () => show(false);
    const keyUp = (event: KeyboardEvent) => {
      if (event.shiftKey && event.key.startsWith("Arrow")) show(true);
    };
    const keyDown = (event: KeyboardEvent) => {
      if (!isSelectionMenuKey(event)) return;
      event.preventDefault();
      show(true);
    };
    const contextMenu = (event: MouseEvent) => {
      if (readTranscriptQuote(window.getSelection(), content))
        event.preventDefault();
    };
    content.addEventListener("pointerup", pointerUp);
    content.addEventListener("keyup", keyUp);
    content.addEventListener("keydown", keyDown);
    content.addEventListener("contextmenu", contextMenu);
    show(takeInitialKeyboard?.() ?? false);
    return () => {
      content.removeEventListener("pointerup", pointerUp);
      content.removeEventListener("keyup", keyUp);
      content.removeEventListener("keydown", keyDown);
      content.removeEventListener("contextmenu", contextMenu);
    };
  }, [content, takeInitialKeyboard]);
  useEffect(() => {
    if (!selected) return;
    const dismiss = () => setSelected(null);
    const inside = (target: EventTarget | null) =>
      toolbar.current?.contains(target as Node) ?? false;
    const keyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      dismiss();
      returnFocus.current?.focus({ preventScroll: true });
    };
    const pointerDown = (event: PointerEvent) => {
      if (!inside(event.target)) dismiss();
    };
    // Keyboard-opened toolbars own focus, so only a mouse selection collapses them.
    const selectionChange = () => {
      if (!selected.keyboard && window.getSelection()?.isCollapsed) dismiss();
    };
    const windowBlur = (event: FocusEvent) => {
      if (event.target === window) dismiss();
    };
    const focusIn = (event: FocusEvent) => {
      if (!scrollport.contains(event.target as Node) && !inside(event.target))
        dismiss();
    };
    // Escape is captured so the toolbar closes before page-level handlers.
    const listeners: [EventTarget, string, EventListener, boolean][] = [
      [document, "keydown", keyDown as EventListener, true],
      [document, "pointerdown", pointerDown as EventListener, false],
      [document, "selectionchange", selectionChange, false],
      [document, "focusin", focusIn as EventListener, false],
      [scrollport, "scroll", dismiss, false],
      [window, "blur", windowBlur as EventListener, false],
      [window, "resize", dismiss, false],
    ];
    for (const [target, type, listener, capture] of listeners)
      target.addEventListener(type, listener, capture);
    return () => {
      for (const [target, type, listener, capture] of listeners)
        target.removeEventListener(type, listener, capture);
    };
  }, [selected, scrollport]);
  useLayoutEffect(() => {
    const element = toolbar.current;
    if (!selected || !element) return;
    const rects = selected.range.getClientRects();
    if (!rects.length) return;
    const column = selected.answer.getBoundingClientRect();
    const viewport = scrollport.getBoundingClientRect();
    const point = selectionPopoverPosition(
      {
        left: rects[0].left,
        top: rects[0].top,
        bottom: rects[rects.length - 1].bottom,
      },
      { left: column.left, top: viewport.top, width: column.width },
      { width: element.offsetWidth || 172, height: element.offsetHeight || 28 },
    );
    setPosition({
      left: column.left + point.left,
      top: viewport.top + point.top,
    });
    if (selected.keyboard) element.querySelector("button")?.focus();
  }, [selected, scrollport]);
  if (!selected) return null;
  const act = (action: (quote: TranscriptQuote) => void) => {
    setSelected(null);
    window.getSelection()?.removeAllRanges();
    action(selected.quote);
  };
  return (
    <div
      ref={toolbar}
      className="chat-selection-popover"
      style={position}
      role="toolbar"
      aria-label="Selection actions"
      onPointerDown={(event) => event.preventDefault()}
      onKeyDown={(event) => {
        if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
        event.preventDefault();
        const buttons = event.currentTarget.querySelectorAll("button");
        buttons[event.key === "ArrowLeft" ? 0 : 1]?.focus();
      }}
    >
      <button type="button" onClick={() => act(onAdd)}>
        Add to chat
      </button>
      <button type="button" onClick={() => act(onSideChat)}>
        Ask in side chat
      </button>
    </div>
  );
}

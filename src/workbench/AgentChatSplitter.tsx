import {
  useState,
  type KeyboardEvent,
  type PointerEvent,
  type RefObject,
} from "react";
import { useSideChatState } from "./sideChatState";
import {
  AGENT_CHAT_SPLIT_MAX,
  AGENT_CHAT_SPLIT_MIN,
  resizeAgentChatSplit,
} from "./agentChatSplit";

export function AgentChatSplitter({
  panesRef,
}: {
  panesRef: RefObject<HTMLDivElement | null>;
}) {
  const split = useSideChatState((state) => state.split);
  const [resizing, setResizing] = useState(false);
  const setSplit = (value: number) =>
    useSideChatState.setState({ split: value });
  function resizeFromPointer(event: PointerEvent<HTMLDivElement>) {
    const bounds = panesRef.current?.getBoundingClientRect();
    if (bounds)
      setSplit(
        resizeAgentChatSplit({
          containerWidth: bounds.width,
          pointerOffset: event.clientX - bounds.left,
        }),
      );
  }
  function endResize(event: PointerEvent<HTMLDivElement>) {
    if (event.currentTarget.hasPointerCapture(event.pointerId))
      event.currentTarget.releasePointerCapture(event.pointerId);
    setResizing(false);
  }
  function resizeByKeyboard(event: KeyboardEvent<HTMLDivElement>) {
    const amount = event.shiftKey ? 10 : 5;
    const next =
      event.key === "ArrowLeft"
        ? split - amount
        : event.key === "ArrowRight"
          ? split + amount
          : event.key === "Home"
            ? AGENT_CHAT_SPLIT_MIN
            : event.key === "End"
              ? AGENT_CHAT_SPLIT_MAX
              : null;
    if (next === null) return;
    event.preventDefault();
    setSplit(
      Math.min(AGENT_CHAT_SPLIT_MAX, Math.max(AGENT_CHAT_SPLIT_MIN, next)),
    );
  }
  return (
    <div
      className={`agent-chat-splitter${resizing ? " agent-chat-splitter--active" : ""}`}
      role="separator"
      aria-label="Resize chat panes"
      aria-orientation="vertical"
      aria-valuemin={AGENT_CHAT_SPLIT_MIN}
      aria-valuemax={AGENT_CHAT_SPLIT_MAX}
      aria-valuenow={split}
      tabIndex={0}
      onKeyDown={resizeByKeyboard}
      onPointerDown={(event) => {
        event.currentTarget.setPointerCapture(event.pointerId);
        setResizing(true);
        resizeFromPointer(event);
      }}
      onPointerMove={(event) => {
        if (resizing) resizeFromPointer(event);
      }}
      onPointerUp={endResize}
      onPointerCancel={endResize}
    />
  );
}

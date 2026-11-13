import type { RefObject } from "react";

import { Splitter } from "../ui/Splitter";
import {
  AGENT_CHAT_SPLIT_MAX,
  AGENT_CHAT_SPLIT_MIN,
  resizeAgentChatSplit,
} from "./agentChatSplit";
import { useSideChatState } from "./sideChatState";

const setSplit = (value: number) =>
  useSideChatState.setState({
    split: Math.min(
      AGENT_CHAT_SPLIT_MAX,
      Math.max(AGENT_CHAT_SPLIT_MIN, value),
    ),
  });

export function AgentChatSplitter({
  panesRef,
}: {
  panesRef: RefObject<HTMLDivElement | null>;
}) {
  const split = useSideChatState((state) => state.split);
  return (
    <Splitter
      ariaLabel="Resize chat panes"
      className="agent-chat-splitter"
      activeClassName="agent-chat-splitter--active"
      value={split}
      min={AGENT_CHAT_SPLIT_MIN}
      max={AGENT_CHAT_SPLIT_MAX}
      onPointerResize={(clientX) => {
        const bounds = panesRef.current?.getBoundingClientRect();
        if (bounds)
          setSplit(
            resizeAgentChatSplit({
              containerWidth: bounds.width,
              pointerOffset: clientX - bounds.left,
            }),
          );
      }}
      onKeyResize={(key, large) => {
        const step = large ? 10 : 5;
        setSplit(
          key === "ArrowLeft"
            ? split - step
            : key === "ArrowRight"
              ? split + step
              : key === "Home"
                ? AGENT_CHAT_SPLIT_MIN
                : AGENT_CHAT_SPLIT_MAX,
        );
      }}
    />
  );
}

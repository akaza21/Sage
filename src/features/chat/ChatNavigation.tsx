import { useCallback, useState, type RefObject } from "react";
import { createPortal } from "react-dom";
import type { TurnProjection } from "../../domain/acp";
import { ChatFindController } from "./ChatFindController";
import { TurnNavigator } from "./TurnNavigator";
import { jumpToChatTurn, type TurnListHost } from "./chatScroll";
import { useChatViewport } from "./useChatViewport";
import { useTurnListNavigation } from "./useTurnListNavigation";

export interface ChatNavigationProps {
  turns: readonly TurnProjection[];
  /** The virtual list's host; null while the Chat renders every Turn. */
  hostRef: RefObject<TurnListHost | null>;
  /** Earlier Turns exist that are not loaded or were discarded. */
  incomplete: boolean;
  /** The Chat shown, so a resumed find never crosses to another Chat. */
  chatId?: string;
}

/**
 * Turn navigator and find-in-chat for one chat pane. Both are overlays of
 * the pane's chat surface, so two side-by-side panes never share state.
 */
export function ChatNavigation({
  turns,
  hostRef,
  incomplete,
  chatId,
}: ChatNavigationProps) {
  const [scrollport, setScrollport] = useState<HTMLElement | null>(null);
  const bind = useCallback(
    (node: HTMLSpanElement | null) =>
      setScrollport(node?.closest<HTMLElement>(".message-list") ?? null),
    [],
  );
  const navigationRef = useTurnListNavigation(scrollport, hostRef, turns);
  const viewport = useChatViewport(scrollport, turns, navigationRef);
  const surface = scrollport?.closest(".chat-surface");
  return (
    <>
      <span hidden ref={bind} />
      {scrollport && surface
        ? createPortal(
            <>
              <TurnNavigator
                turns={turns}
                visibleIds={viewport.visibleIds}
                top={viewport.top}
                height={viewport.height}
                onJump={(id) =>
                  jumpToChatTurn(scrollport, id, navigationRef.current)
                }
              />
              <ChatFindController
                scrollport={scrollport}
                navigationRef={navigationRef}
                turns={turns}
                incomplete={incomplete}
                chatId={chatId}
              />
            </>,
            surface,
          )
        : null}
    </>
  );
}

import { useCallback, useState, type RefObject } from "react";
import { createPortal } from "react-dom";
import { useStore } from "zustand";
import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import { type TurnProjection } from "../../domain/acp";
import { ChatFindController } from "./ChatFindController";
import { TurnNavigator } from "./TurnNavigator";
import { jumpToChatTurn, type TurnListHost } from "./chatScroll";
import { useChatViewport } from "./useChatViewport";
import { useTurnListNavigation } from "./useTurnListNavigation";
import { lazyParts } from "./lazyParts";
import type * as ImportChunk from "./ImportedHistory";

const ImportPart = lazyParts<typeof ImportChunk>(
  () => import("./ImportedHistory"),
);

export interface ChatNavigationProps {
  turns: readonly TurnProjection[];
  /** Null while the Chat renders every Turn. */
  hostRef: RefObject<TurnListHost | null>;
  incomplete: boolean;
  chatId?: string;
}

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
  const imported = useStore(
    acpProjectionStoreFor(chatId),
    (state) => state.page?.importedHistory,
  );
  return (
    <>
      <span hidden ref={bind} />
      {imported && chatId && !incomplete ? (
        <ImportPart part="ImportedHistory" chatId={chatId} />
      ) : null}
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

export {
  AcpHistoryNotices,
  WorkspaceTrackingNotice,
} from "./AcpHistoryNotices";

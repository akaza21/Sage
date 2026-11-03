import { flushSync } from "react-dom";

import { acpProjectionStoreFor } from "../../../src/app/acpProjectionRegistry";
import { installChatChromeFixture } from "./chatChromeFixture";

/** Deliver old-layout scrolling after a synchronous transcript replacement. */
export function replaceChatChromeWithQueuedScroll() {
  const transcript = document.querySelector<HTMLElement>(".message-list");
  const chatId = document.querySelector<HTMLElement>(".acp-chat-projection")
    ?.dataset.chatId;
  if (!transcript || !chatId) throw new Error("Chat transcript is not ready");
  const store = acpProjectionStoreFor(chatId);
  const before = store.getState().page?.projectionRevision;
  transcript.scrollTop -= 1;
  transcript.scrollTop = transcript.scrollHeight;
  // Pin the interleaving: React commits the new content before the browser
  // delivers the scroll event queued against the previous transcript layout.
  flushSync(() =>
    installChatChromeFixture("oh-my-pi", false, "completed", false),
  );
  return { before, after: store.getState().page?.projectionRevision };
}

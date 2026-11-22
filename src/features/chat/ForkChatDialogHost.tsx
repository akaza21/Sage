import { useState, type ReactNode, type RefObject } from "react";

import {
  ConversationRequestContext,
  type ConversationRequest,
} from "./forkChatRequest";
import { NoticePart } from "./lazyParts";

// A separate host: the source answer may disappear during a project switch.
export function ForkChatDialogHost({
  children,
  focusRoot,
}: {
  children: ReactNode;
  focusRoot: RefObject<HTMLDivElement | null>;
}) {
  const [request, setRequest] = useState<ConversationRequest | null>(null);
  return (
    <ConversationRequestContext.Provider value={setRequest}>
      {children}
      {request && (
        <NoticePart
          part="ConversationView"
          view="ConversationDialog"
          request={request}
          focusRoot={focusRoot}
          onClose={() => setRequest(null)}
          onLoadFailed={() => setRequest(null)}
        />
      )}
    </ConversationRequestContext.Provider>
  );
}

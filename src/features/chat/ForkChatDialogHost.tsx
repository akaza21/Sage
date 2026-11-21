import { useState, type ReactNode, type RefObject } from "react";

import {
  ConversationRequestContext,
  type ConversationRequest,
} from "./forkChatRequest";
import { ConversationPart } from "./lazyParts";

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
        <ConversationPart
          key={"kind" in request ? "rewind" : "fork"}
          part={"kind" in request ? "RewindChatDialog" : "ForkChatDialog"}
          {...request}
          focusRoot={focusRoot}
          onClose={() => setRequest(null)}
        />
      )}
    </ConversationRequestContext.Provider>
  );
}

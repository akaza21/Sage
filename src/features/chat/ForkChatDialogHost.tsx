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
  const part =
    request && "kind" in request ? "RewindChatDialog" : "ForkChatDialog";
  return (
    <ConversationRequestContext.Provider value={setRequest}>
      {children}
      {request && (
        <ConversationPart
          key={part}
          part={part}
          {...request}
          focusRoot={focusRoot}
          onClose={() => setRequest(null)}
        />
      )}
    </ConversationRequestContext.Provider>
  );
}

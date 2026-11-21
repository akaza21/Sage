import { useState, type ReactNode, type RefObject } from "react";

import {
  ForkChatRequestContext,
  RewindChatRequestContext,
  type RewindChatRequest,
  type ForkChatRequest,
} from "./forkChatRequest";
import { ConversationPart, ForkPart } from "./lazyParts";

// A separate host: the source answer may disappear during a project switch.
export function ForkChatDialogHost({
  children,
  focusRoot,
}: {
  children: ReactNode;
  focusRoot: RefObject<HTMLDivElement | null>;
}) {
  const [request, setRequest] = useState<ForkChatRequest | null>(null);
  const [rewind, setRewind] = useState<RewindChatRequest | null>(null);
  return (
    <ForkChatRequestContext.Provider value={setRequest}>
      <RewindChatRequestContext.Provider value={setRewind}>
        {children}
        {request && (
          <ForkPart
            part="ForkChatDialog"
            {...request}
            focusRoot={focusRoot}
            onClose={() => setRequest(null)}
          />
        )}
        {rewind && (
          <ConversationPart
            part="RewindChatDialog"
            {...rewind}
            onClose={() => setRewind(null)}
          />
        )}
      </RewindChatRequestContext.Provider>
    </ForkChatRequestContext.Provider>
  );
}

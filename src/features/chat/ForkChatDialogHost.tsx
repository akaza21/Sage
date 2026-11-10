import { useState, type ReactNode, type RefObject } from "react";

import {
  ForkChatRequestContext,
  type ForkChatRequest,
} from "./forkChatRequest";
import { ForkPart } from "./lazyParts";

/** The answer may disappear during a native project transition. Its dialog
 * stays here until the fork commits or the caller dismisses its refusal. */
export function ForkChatDialogHost({
  children,
  focusRoot,
}: {
  children: ReactNode;
  focusRoot: RefObject<HTMLDivElement | null>;
}) {
  const [request, setRequest] = useState<ForkChatRequest | null>(null);
  return (
    <ForkChatRequestContext.Provider value={setRequest}>
      {children}
      {request && (
        <ForkPart
          part="ForkChatDialog"
          chatId={request.chatId}
          turnId={request.turnId}
          keyboard={request.keyboard}
          focusRoot={focusRoot}
          onClose={() => setRequest(null)}
        />
      )}
    </ForkChatRequestContext.Provider>
  );
}

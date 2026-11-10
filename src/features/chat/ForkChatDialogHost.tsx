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
  focusRoot?: RefObject<HTMLDivElement | null>;
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
          onClose={() => {
            setRequest(null);
            // The transition may have replaced the answer's button. Keep
            // only its live container ref, never a detached answer subtree.
            queueMicrotask(() => {
              const buttons =
                focusRoot?.current?.querySelectorAll<HTMLButtonElement>(
                  'button[aria-label="More actions"]',
                );
              buttons?.item(buttons.length - 1)?.focus();
            });
          }}
        />
      )}
    </ForkChatRequestContext.Provider>
  );
}

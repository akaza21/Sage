import { useState, type ReactNode, type RefObject } from "react";

import {
  ForkChatRequestContext,
  type ForkChatRequest,
} from "./forkChatRequest";
import { ForkPart } from "./lazyParts";

// A separate host: the source answer may disappear during a project switch.
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
          {...request}
          focusRoot={focusRoot}
          onClose={() => setRequest(null)}
        />
      )}
    </ForkChatRequestContext.Provider>
  );
}

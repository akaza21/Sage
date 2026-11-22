export { ForkDivider, InheritedHistory } from "./ForkHistory";
export { SessionSurfacePanel } from "./acp/SessionSurfacePanel";
export { AnswerMoreActions } from "./ForkChat";

import type { ComponentProps } from "react";
import { ForkChatDialog } from "./ForkChat";
import { RewindChatDialog } from "./RewindChatDialog";
import type { ConversationRequest } from "./forkChatRequest";

export function ConversationDialog({
  request,
  ...props
}: {
  request: ConversationRequest;
} & Pick<ComponentProps<typeof ForkChatDialog>, "focusRoot" | "onClose">) {
  return "kind" in request ? (
    <RewindChatDialog {...request} {...props} />
  ) : (
    <ForkChatDialog {...request} {...props} />
  );
}

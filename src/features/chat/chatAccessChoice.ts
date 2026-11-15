import {
  chooseChatAccess,
  PERMISSIVE_ACCESS,
} from "../../app/sessionAccessActions";
import type { SessionAccessMode } from "../../domain/sessionAccessMode";
import type { AccessView } from "./ComposerAccessControl";

export const CONFIRMED_ACCESS: readonly SessionAccessMode[] = [
  "workspaceWrite",
  ...PERMISSIVE_ACCESS,
];

export function chooseAccess(
  { sessionId, onDraft }: Pick<AccessView, "sessionId" | "onDraft">,
  mode: SessionAccessMode,
) {
  if (sessionId) void chooseChatAccess(sessionId, mode);
  else onDraft?.(mode);
}

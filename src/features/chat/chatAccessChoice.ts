import {
  chooseChatAccess,
  PERMISSIVE_ACCESS,
} from "../../app/sessionAccessActions";
import type { SessionAccessMode } from "../../domain/sessionAccessMode";
import type { AccessView } from "./ComposerAccessControl";

/** Modes confirmed in the access menu before they apply. */
export const CONFIRMED_ACCESS: readonly SessionAccessMode[] = [
  "workspaceWrite",
  ...PERMISSIVE_ACCESS,
];

let requested: SessionAccessMode | null = null;

/** Asks the access menu, about to open, for this mode's confirmation. */
export function requestAccessConfirmation(mode: SessionAccessMode) {
  requested = mode;
}

/** The confirmation asked for outside the menu, until the menu takes it. */
export function requestedAccessConfirmation() {
  return requested;
}

export function clearAccessConfirmation() {
  requested = null;
}

/** Gives the chat a mode that needs no native token, or drafts it. */
export function chooseAccess(
  { sessionId, onDraft }: Pick<AccessView, "sessionId" | "onDraft">,
  mode: SessionAccessMode,
) {
  if (sessionId) void chooseChatAccess(sessionId, mode);
  else onDraft?.(mode);
}

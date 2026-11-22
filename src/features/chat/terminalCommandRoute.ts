import { useWorkbenchStore } from "../../app/store";
import type { AcpCommandList } from "../../services/acpCommandsDesktop";
import { AcpDesktopError } from "../../services/acpDesktop";
import { decodeTerminalHandoff } from "../../services/terminalHandoff";
import { interceptTerminalCommand } from "./interceptTerminalCommand";
import {
  TERMINAL_COMMAND_EVENT,
  type TerminalCommandEvent,
} from "./terminalCommandEvents";

function commandField(chatId: string): HTMLTextAreaElement | null {
  const field =
    document.getElementById(`prompt-${chatId}`) ??
    (useWorkbenchStore.getState().activeTabId === chatId
      ? document.getElementById("prompt")
      : null);
  return field instanceof HTMLTextAreaElement ? field : null;
}

/**
 * Loaded only for an unreported `/name` on a binding with a terminal handoff.
 * Returns true when the field's command controller took the command, so
 * nothing is sent; false sends the prompt as text, as before.
 */
export function routeTerminalCommand(
  chatId: string,
  prompt: string,
  name: string,
  list: AcpCommandList,
): boolean {
  const field = commandField(chatId);
  // Only a prompt the field still shows is caught; a queued send keeps its route.
  if (!field || field.value.trim() !== prompt) return false;
  // "Send as text anyway" consents to exactly one send of this input.
  const consent = field.dataset.terminalText;
  delete field.dataset.terminalText;
  if (consent === prompt) return false;
  // A malformed handoff only disables the catch, never the command list.
  const handoff = decodeTerminalHandoff(list.terminalHandoff);
  if (!handoff) return false;
  const action = interceptTerminalCommand(
    name,
    list.commands,
    handoff.terminalOnly,
    handoff.controls,
  );
  if (action.kind === "send") return false;
  const event: TerminalCommandEvent = new CustomEvent(TERMINAL_COMMAND_EVENT, {
    cancelable: true,
    detail: {
      input: prompt,
      name,
      handoff,
      ...(action.kind === "control" ? { control: action.control } : {}),
    },
  });
  // The field's command controller cancels it to own the choice.
  if (field.dispatchEvent(event))
    throw new AcpDesktopError({
      kind: "invalidRequest",
      message: `/${name} needs ${handoff.label}'s terminal, so nothing was sent.`,
    });
  return true;
}

import { useContext, useEffect, useState, type RefObject } from "react";
import { useWorkbenchStore } from "../../app/store";
import type { AcpCommandList } from "../../services/acpCommandsDesktop";
import { AcpDesktopError } from "../../services/acpDesktop";
import { decodeTerminalHandoff } from "../../services/terminalHandoff";
import { interceptTerminalCommand } from "./interceptTerminalCommand";
import type { AcpNativeCommandControl } from "../../domain/acpCommandPlacement";
import type { AcpCommandPanel } from "../../domain/acpCommand";
import { Notice } from "../../ui/Notice";
import { AgentToolsContext } from "../../workbench/AgentToolsProvider";
import { prefillTerminal } from "../../workbench/terminalPrefill";
import {
  TERMINAL_COMMAND_EVENT,
  type TerminalCommandEvent,
  type TerminalCommandRequest,
} from "./terminalCommandEvents";

const PANELS: Partial<Record<AcpNativeCommandControl, AcpCommandPanel>> = {
  mcpPanel: "mcp",
  usagePanel: "status",
};
// The composer toolbar triggers a control opens, by their own classes.
const TRIGGERS: Partial<Record<AcpNativeCommandControl, string>> = {
  modelPicker: ".composer__model-trigger",
  accessMenu: ".composer__access-label",
};

/** Opens the composer's own control for a caught command, when it is shown. */
function openControl(
  field: HTMLTextAreaElement,
  control: AcpNativeCommandControl,
  onPanel: (panel: AcpCommandPanel) => void,
  onClear: () => void,
): boolean {
  const panel = PANELS[control];
  const selector = TRIGGERS[control];
  const trigger = selector
    ? field.form?.querySelector<HTMLButtonElement>(selector)
    : null;
  if (!panel && (!trigger || trigger.disabled)) return false;
  onClear();
  if (panel) onPanel(panel);
  else trigger!.click();
  return true;
}

/**
 * The one pending choice for a typed terminal-only command, loaded only when
 * one is caught: a native control replaces it when the composer shows one;
 * otherwise the user picks the terminal or plain text. Nothing is sent before.
 */
export function ComposerTerminalCommand({
  boundChatId,
  fieldRef,
  onClear,
  onDone,
  onPanel,
  request,
}: {
  boundChatId?: string;
  fieldRef: RefObject<HTMLTextAreaElement | null>;
  /** Removes the typed command the way an Undo can restore. */
  onClear: () => void;
  onDone: () => void;
  onPanel: (panel: AcpCommandPanel) => void;
  request: TerminalCommandRequest;
}) {
  const { launch, terminalAvailable } = useContext(AgentToolsContext);
  const { control, handoff, input, name } = request;
  const [shown, setShown] = useState(!control);
  useEffect(() => {
    const field = fieldRef.current;
    if (!control) return;
    if (field && openControl(field, control, onPanel, onClear)) onDone();
    else setShown(true);
  }, [control, fieldRef, onClear, onDone, onPanel]);
  if (!shown) return null;
  const openInTerminal =
    terminalAvailable && boundChatId ? (
      <button
        type="button"
        className="notice__action"
        onClick={() => {
          onDone();
          onClear();
          prefillTerminal(handoff.command, () =>
            launch("terminal", boundChatId),
          );
        }}
      >
        Open in terminal
      </button>
    ) : null;
  return (
    <Notice
      identity={`terminal-command:${name}`}
      severity="info"
      message={
        <>
          {`/${name} needs ${handoff.label}'s terminal.`} {openInTerminal}
        </>
      }
      action={{
        label: "Send as text anyway",
        onClick: () => {
          const field = fieldRef.current;
          onDone();
          if (!field) return;
          field.dataset.terminalText = input;
          field.form?.requestSubmit();
        },
      }}
    />
  );
}

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
// eslint-disable-next-line react-refresh/only-export-components -- the send route and the choice it opens load as one chunk
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

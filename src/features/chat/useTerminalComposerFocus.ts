import { useEffect, useRef, type RefObject } from "react";
import type { AcpChatPage, TurnProjection } from "../../domain/acp";
import { terminalTurnStates } from "./acp";
export function useTerminalComposerFocus({
  acpPage,
  activeAcpTurn,
  focused,
  actionBusy,
  projectTransitioning,
  canPrompt,
  surfaceRef,
  composerRef,
}: {
  acpPage: AcpChatPage | null | undefined;
  activeAcpTurn: TurnProjection | null | undefined;
  focused: boolean;
  actionBusy: boolean;
  projectTransitioning: boolean;
  canPrompt: boolean;
  surfaceRef: RefObject<HTMLElement | null>;
  composerRef: RefObject<HTMLTextAreaElement | null>;
}) {
  const priorActiveTurnId = useRef<TurnProjection["id"] | null>(null);
  const terminalFocusTurnId = useRef<TurnProjection["id"] | null>(null);
  useEffect(() => {
    const priorId = priorActiveTurnId.current;
    const priorTurn = priorId
      ? acpPage?.turns.find((turn) => turn.id === priorId)
      : null;
    if (priorId && priorTurn && terminalTurnStates.has(priorTurn.state)) {
      terminalFocusTurnId.current = priorId;
      priorActiveTurnId.current = activeAcpTurn?.id ?? null;
    } else if (activeAcpTurn) {
      priorActiveTurnId.current = activeAcpTurn.id;
    } else if (priorId && !priorTurn) {
      priorActiveTurnId.current = null;
    }
    if (
      focused &&
      terminalFocusTurnId.current &&
      !actionBusy &&
      !projectTransitioning
    ) {
      const surface = surfaceRef.current;
      const activeElement = document.activeElement;
      const preserveFocus =
        activeElement !== surface && surface?.contains(activeElement);
      const composer = composerRef.current;
      const statuses =
        surface?.querySelectorAll<HTMLElement>("[data-turn-status]");
      const terminalStatus = [...(statuses ?? [])].find(
        (status) => status.dataset.turnStatus === terminalFocusTurnId.current,
      );
      const target = composer && !composer.disabled ? composer : terminalStatus;
      if (!preserveFocus) target?.focus();
      if (target || preserveFocus) terminalFocusTurnId.current = null;
    }
  }, [
    acpPage,
    activeAcpTurn,
    actionBusy,
    canPrompt,
    focused,
    projectTransitioning,
  ]);
}

import { useEffect, useRef, type RefObject } from "react";
import type { AcpChatPage, TurnProjection } from "../../domain/acp";
import { terminalTurnStates } from "./acp";

/**
 * Once the chat's running turn ends, focus returns to the composer, or to
 * that turn's status while the composer is disabled, unless the person has
 * since moved focus elsewhere in the chat.
 */
export function useTerminalTurnFocus({
  page,
  activeTurn,
  actionBusy,
  canPrompt,
  focused,
  projectTransitioning,
  surfaceRef,
  composerRef,
}: {
  page: AcpChatPage | null | undefined;
  activeTurn: TurnProjection | null | undefined;
  actionBusy: boolean;
  canPrompt: boolean;
  focused: boolean;
  projectTransitioning: boolean;
  surfaceRef: RefObject<HTMLElement | null>;
  composerRef: RefObject<HTMLTextAreaElement | null>;
}) {
  const priorActiveTurnId = useRef<TurnProjection["id"] | null>(null);
  const terminalFocusTurnId = useRef<TurnProjection["id"] | null>(null);
  useEffect(() => {
    const priorId = priorActiveTurnId.current;
    const priorTurn = priorId
      ? page?.turns.find((turn) => turn.id === priorId)
      : null;
    if (priorId && priorTurn && terminalTurnStates.has(priorTurn.state)) {
      terminalFocusTurnId.current = priorId;
      priorActiveTurnId.current = activeTurn?.id ?? null;
    } else if (activeTurn) {
      priorActiveTurnId.current = activeTurn.id;
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
    page,
    activeTurn,
    actionBusy,
    canPrompt,
    focused,
    projectTransitioning,
    surfaceRef,
    composerRef,
  ]);
}

import { chooseChatAccess } from "../../app/sessionAccessActions";
import type { SessionAccessMode } from "../../domain/sessionAccessMode";
import { Lightbulb } from "../../ui/leanIcons";
import { ChatAccessView } from "./ChatAccessView";
import type { ComposerAccessControlProps } from "./ComposerAccessControl";

/** The chat whose access the "+" menu's Plan mode switches. */
export type PlanAccessTarget = Pick<
  ComposerAccessControlProps,
  "bindingId" | "chatId" | "sessionConfig" | "capabilities"
>;

/**
 * Plan mode where the harness's access modes carry its session modes: on
 * gives the chat the Plan access mode, off its default, through the access
 * menu's own path (the live chat's access, or the draft its session takes).
 */
export function ComposerPlanAccess({
  target,
  disabled,
  onClose,
}: {
  target: PlanAccessTarget;
  disabled: boolean;
  onClose: () => void;
}) {
  return (
    <ChatAccessView {...target} error={null}>
      {({ candidates, effective, defaultMode, sessionId, onDraft }) => {
        if (!candidates.some((mode) => mode.id === "plan")) return null;
        const on = effective === "plan";
        const next = on ? defaultMode : "plan";
        const choose = (mode: SessionAccessMode) =>
          sessionId ? void chooseChatAccess(sessionId, mode) : onDraft?.(mode);
        const description = on ? "Turn plan mode off" : "Turn plan mode on";
        return (
          <button
            type="button"
            role="menuitemcheckbox"
            className="menu-item composer__add-action"
            aria-label="Plan mode"
            aria-description={description}
            aria-checked={on}
            disabled={disabled || !next || next === effective}
            tabIndex={-1}
            onClick={() => {
              onClose();
              if (next) choose(next);
            }}
          >
            <Lightbulb size={14} aria-hidden="true" />
            <span>Plan mode</span>
            <span className="composer__action-description">{description}</span>
          </button>
        );
      }}
    </ChatAccessView>
  );
}

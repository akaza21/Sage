import { Lightbulb } from "../../ui/leanIcons";
import { ChatAccessView } from "./ChatAccessView";
import {
  chooseAccess,
  CONFIRMED_ACCESS,
  requestAccessConfirmation,
} from "./chatAccessChoice";
import type { ComposerAccessControlProps } from "./ComposerAccessControl";

export type PlanAccessTarget = Pick<
  ComposerAccessControlProps,
  "bindingId" | "chatId" | "sessionConfig" | "capabilities"
> & {
  onOpenAccess: () => void;
};

/**
 * Plan mode where the harness's access modes carry its session modes: on
 * gives the chat the Plan access mode, off its default, through the access
 * menu's own path, including its confirmation for a default that needs one.
 */
export function ComposerPlanAccess({
  target: { onOpenAccess, ...target },
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
              if (next && CONFIRMED_ACCESS.includes(next)) {
                requestAccessConfirmation(next);
                onOpenAccess();
              } else if (next) chooseAccess({ sessionId, onDraft }, next);
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

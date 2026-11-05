import { Check, Sparkles, Target } from "../../ui/leanIcons";
import {
  useLayoutEffect,
  type ComponentProps,
  type ReactNode,
  type RefObject,
} from "react";
import { MenuPopover } from "../../ui/MenuPopover";
import type {
  AcpConfigValue,
  AcpSessionConfig,
} from "../../domain/acpSessionConfig";
import { ChatSessionConfigControls } from "./ChatSessionConfigControls";
import { sessionModeActions } from "../../domain/acpSessionConfigControls";

export function ComposerOptionsMenu({
  id,
  triggerRef,
  onDismiss,
  ...actions
}: ComponentProps<typeof ComposerAddActions> & {
  id: string;
  triggerRef: RefObject<HTMLButtonElement | null>;
  onDismiss: () => void;
}) {
  const overflowKey =
    Boolean(actions.projectAction) + ":" + Boolean(actions.onOpenEffort);
  useLayoutEffect(() => {
    // Live fitting can remove the focused overflow item while fixed actions remain.
    if (document.activeElement === document.body) {
      const panel = document.getElementById(id);
      const target =
        panel?.querySelector<HTMLButtonElement>("button:not(:disabled)") ??
        panel;
      if (target) {
        if (target === panel) target.tabIndex = -1;
        target.focus();
      }
    }
  }, [id, overflowKey]);
  return (
    <MenuPopover
      id={id}
      className="composer__popover composer__options"
      ariaLabel="Composer options menu"
      focusKey="options"
      restoreFocusOnTab
      open
      triggerRef={triggerRef}
      onDismiss={onDismiss}
      gap={8}
    >
      <ComposerAddActions {...actions} />
    </MenuPopover>
  );
}

export function ComposerAddActions({
  sessionConfig,
  projectAction,
  pending,
  disabled,
  imageAction,
  onOpenGoal,
  onOpenEffort,
  onClose,
  onSetSessionMode,
  onSetSessionConfigOption,
}: {
  sessionConfig: AcpSessionConfig | null;
  projectAction?: ReactNode;
  pending: boolean;
  disabled: boolean;
  imageAction?: { disabled: boolean; onChoose: () => void };
  onOpenGoal?: () => void;
  onOpenEffort?: () => void;
  onClose: () => void;
  onSetSessionMode: (id: string) => Promise<unknown>;
  onSetSessionConfigOption: (
    id: string,
    value: AcpConfigValue,
  ) => Promise<unknown>;
}) {
  const modes = sessionModeActions(sessionConfig);
  return (
    <>
      <p className="menu-group-label composer__add-heading" aria-hidden="true">
        Add
      </p>
      {projectAction}
      {imageAction ? (
        <button
          type="button"
          role="menuitem"
          className="menu-item"
          disabled={imageAction.disabled}
          tabIndex={-1}
          onClick={() => {
            imageAction.onChoose();
            onClose();
          }}
        >
          Add images
        </button>
      ) : null}
      {onOpenGoal ? (
        <button
          type="button"
          role="menuitem"
          className="menu-item composer__add-action"
          aria-label="Goal"
          aria-description="View the current goal"
          tabIndex={-1}
          onClick={() => {
            onClose();
            onOpenGoal();
          }}
        >
          <Target size={14} aria-hidden="true" />
          <span>Goal</span>
          <span className="composer__action-description">
            View the current goal
          </span>
        </button>
      ) : null}
      {onOpenEffort ? (
        <button
          type="button"
          role="menuitem"
          className="menu-item"
          tabIndex={-1}
          onClick={onOpenEffort}
        >
          Thinking level
        </button>
      ) : null}
      <ChatSessionConfigControls
        sessionConfig={sessionConfig}
        group="other"
        pending={pending}
        disabled={disabled}
        onSetSessionConfigOption={(id, value) => {
          onClose();
          return onSetSessionConfigOption(id, value);
        }}
      />
      {modes.length ? (
        <div role="group" aria-label="Session mode">
          {modes.map((mode) => {
            const plan = mode.name.toLocaleLowerCase() === "plan";
            const label = plan ? "Plan mode" : mode.name || mode.id;
            const selected = mode.id === sessionConfig?.modes?.currentModeId;
            const description = plan
              ? selected
                ? "Plan mode is on"
                : "Plan mode is off"
              : mode.description || undefined;
            return (
              <button
                key={mode.id}
                type="button"
                role="menuitemradio"
                className="menu-item composer__add-action"
                aria-label={label}
                aria-description={description}
                aria-checked={selected}
                disabled={disabled || pending}
                tabIndex={-1}
                onClick={() => {
                  onClose();
                  // Modes are exclusive; choosing the current one sends nothing.
                  if (!selected) void onSetSessionMode(mode.id);
                }}
              >
                {plan ? <Sparkles size={14} aria-hidden="true" /> : null}
                <span>{label}</span>
                {description ? (
                  <span className="composer__action-description">
                    {description}
                  </span>
                ) : null}
                {selected ? <Check size={14} aria-hidden="true" /> : null}
              </button>
            );
          })}
        </div>
      ) : null}
    </>
  );
}

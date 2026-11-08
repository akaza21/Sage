import { Check, Sparkles, Target } from "../../ui/leanIcons";
import {
  useLayoutEffect,
  useRef,
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
import {
  sessionConfigControls,
  sessionModeActions,
  flattenChoices,
} from "../../domain/acpSessionConfigControls";
import { withoutGovernedModes } from "../../domain/governedSessionModes";
import type { SessionAccessModeCapability } from "../../domain/sessionAccessMode";

export function ComposerOptionsMenu({
  id,
  triggerRef,
  onDismiss,
  selectionKey,
  canSelectEffort,
  focused,
  onModeRemoved,
  accessModes,
  ...given
}: ComponentProps<typeof ComposerAddActions> & {
  /** The harness's access modes; those carrying session modes own them. */
  accessModes?: readonly SessionAccessModeCapability[];
  id: string;
  selectionKey: string;
  canSelectEffort: boolean;
  focused: boolean;
  onModeRemoved: () => void;
  triggerRef: RefObject<HTMLButtonElement | null>;
  onDismiss: () => void;
}) {
  const actions = {
    ...given,
    sessionConfig: withoutGovernedModes(given.sessionConfig, accessModes),
  };
  const actionsKey = JSON.stringify([
    selectionKey,
    actions.sessionConfig?.sageAgentSessionId,
    Boolean(actions.imageAction),
    Boolean(actions.onOpenGoal),
    canSelectEffort,
    sessionModeActions(actions.sessionConfig).map((mode) => mode.id),
    sessionConfigControls(actions.sessionConfig, "other").map((option) => [
      option.id,
      option.type,
      flattenChoices(option).map((choice) => choice.value),
    ]),
  ]);
  const previousActionsKey = useRef(actionsKey);
  useLayoutEffect(() => {
    if (previousActionsKey.current !== actionsKey) {
      const element = document.activeElement;
      const menuHadFocus =
        element === document.body ||
        Boolean(element && document.getElementById(id)?.contains(element));
      onDismiss();
      if (focused && menuHadFocus) onModeRemoved();
    }
    previousActionsKey.current = actionsKey;
  }, [actionsKey, focused, id, onDismiss, onModeRemoved]);
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
          data-overflow-control="effort"
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

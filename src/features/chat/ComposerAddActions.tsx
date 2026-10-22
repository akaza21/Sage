import { Check, Sparkles, Target } from "../../ui/leanIcons";
import type {
  AcpConfigValue,
  AcpSessionConfig,
} from "../../domain/acpSessionConfig";
import { ChatSessionConfigControls } from "./ChatSessionConfigControls";

export function ComposerAddActions({
  sessionConfig,
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
  const modes = sessionConfig?.removed
    ? []
    : (sessionConfig?.modes?.availableModes ?? []);
  return (
    <>
      <p className="menu-group-label" aria-hidden="true">
        Add
      </p>
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
            const description =
              mode.description ||
              (plan ? "Turn plan mode on" : `Use ${label} mode`);
            const selected = mode.id === sessionConfig?.modes?.currentModeId;
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
                  void onSetSessionMode(mode.id);
                }}
              >
                <Sparkles size={14} aria-hidden="true" />
                <span>{label}</span>
                <span className="composer__action-description">
                  {description}
                </span>
                {selected ? <Check size={14} aria-hidden="true" /> : null}
              </button>
            );
          })}
        </div>
      ) : null}
    </>
  );
}

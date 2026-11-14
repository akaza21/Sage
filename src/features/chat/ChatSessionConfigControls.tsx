import { Check } from "../../ui/leanIcons";

import type {
  AcpConfigOption,
  AcpConfigValue,
  AcpSessionConfig,
} from "../../domain/acpSessionConfig";
import {
  flattenChoices,
  sessionConfigControls,
} from "../../domain/acpSessionConfigControls";

interface Props {
  sessionConfig: AcpSessionConfig | null;
  group?: "model" | "other";
  pending: boolean;
  disabled?: boolean;
  onSetSessionConfigOption: (
    configId: string,
    value: AcpConfigValue,
  ) => Promise<unknown>;
}

export function ChatSessionConfigControls({
  sessionConfig,
  group,
  pending,
  disabled = false,
  onSetSessionConfigOption,
}: Props) {
  const options = sessionConfigControls(sessionConfig, group);
  if (options.length === 0) return null;
  return (
    <>
      {options.map((option) => (
        <SessionConfigControl
          key={option.id}
          option={option}
          restored={sessionConfig?.restored}
          disabled={disabled || pending}
          onChange={onSetSessionConfigOption}
        />
      ))}
    </>
  );
}

function SessionConfigControl({
  option,
  restored,
  disabled,
  onChange,
}: {
  option: AcpConfigOption;
  restored?: string;
  disabled: boolean;
  onChange: (configId: string, value: AcpConfigValue) => void;
}) {
  const controlLabel = `Session ${option.name.toLocaleLowerCase()}`;
  return (
    <div
      role="group"
      aria-label={controlLabel}
      aria-description={restored}
      title={restored}
      data-restored={restored ? true : undefined}
    >
      {option.type === "unsupported" ? (
        <span className="menu-empty" aria-disabled="true">
          {option.name}
        </span>
      ) : option.type === "boolean" ? (
        <button
          type="button"
          role="menuitemcheckbox"
          className="menu-item"
          aria-label={controlLabel}
          aria-checked={option.currentValue}
          disabled={disabled}
          tabIndex={-1}
          onClick={() =>
            onChange(option.id, {
              type: "boolean",
              value: !option.currentValue,
            })
          }
        >
          {option.name}
          {option.currentValue ? <Check size={14} aria-hidden="true" /> : null}
        </button>
      ) : (
        <>
          <p className="menu-empty" aria-hidden="true">
            {option.name}
          </p>
          {flattenChoices(option).map((choice) => (
            <button
              key={choice.value}
              type="button"
              role="menuitemradio"
              className="menu-item"
              aria-checked={choice.value === option.currentValue}
              disabled={disabled}
              tabIndex={-1}
              onClick={() => onChange(option.id, { value: choice.value })}
            >
              <span>{choice.name || choice.value}</span>
              {choice.value === option.currentValue ? (
                <Check size={14} aria-hidden="true" />
              ) : null}
            </button>
          ))}
        </>
      )}
    </div>
  );
}

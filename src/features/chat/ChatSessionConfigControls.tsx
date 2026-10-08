import { SlidersHorizontal } from "../../ui/leanIcons";
import { useCallback, useState } from "react";

import type {
  AcpConfigOption,
  AcpConfigValue,
  AcpSessionConfig,
} from "../../domain/acpSessionConfig";
import {
  flattenChoices,
  sessionConfigControls,
} from "../../domain/acpSessionConfigControls";
import { SearchablePicker } from "../../ui/SearchablePicker";

interface Props {
  sessionConfig: AcpSessionConfig | null;
  group?: "model" | "other";
  pending: boolean;
  disabled?: boolean;
  error: string | null;
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
  error,
  onSetSessionConfigOption,
}: Props) {
  const [lastChangedId, setLastChangedId] = useState<string | null>(null);
  const options = sessionConfigControls(sessionConfig)
    .filter((option) => {
      if (!group) return true;
      const modelRelated = ["model", "effort"].includes(
        (option.category ?? option.id).toLocaleLowerCase(),
      );
      return group === "model" ? modelRelated : !modelRelated;
    })
    .sort((left, right) => configPriority(left.id) - configPriority(right.id));
  const change = useCallback(
    (configId: string, value: AcpConfigValue) => {
      setLastChangedId(configId);
      void onSetSessionConfigOption(configId, value);
    },
    [onSetSessionConfigOption],
  );
  if (options.length === 0) return null;
  return (
    <>
      {options.map((option) => (
        <SessionConfigControl
          key={option.id}
          option={option}
          pending={pending && lastChangedId === option.id}
          disabled={disabled || pending}
          error={lastChangedId === option.id ? error : null}
          onChange={change}
        />
      ))}
    </>
  );
}

function configPriority(id: string): number {
  const normalized = id.toLocaleLowerCase();
  if (normalized === "model") return 0;
  if (normalized === "effort") return 1;
  return 2;
}

function SessionConfigControl({
  option,
  pending,
  disabled,
  error,
  onChange,
}: {
  option: AcpConfigOption;
  pending: boolean;
  disabled: boolean;
  error: string | null;
  onChange: (configId: string, value: AcpConfigValue) => void;
}) {
  const controlLabel = `Session ${option.name.toLocaleLowerCase()}`;
  return (
    <label className="select-control select-control--chip">
      <SlidersHorizontal
        className="select-control__icon"
        size={14}
        aria-hidden="true"
      />
      {option.type === "unsupported" ? (
        <span className="select-control__unsupported" aria-disabled="true">
          {option.name}
        </span>
      ) : option.type === "boolean" ? (
        <input
          type="checkbox"
          aria-label={controlLabel}
          checked={option.currentValue}
          disabled={disabled}
          onChange={(event) =>
            onChange(option.id, {
              type: "boolean",
              value: event.target.checked,
            })
          }
        />
      ) : (
        <SearchablePicker
          ariaLabel={controlLabel}
          candidates={flattenChoices(option).map((choice) => ({
            id: choice.value,
            label: choice.name || choice.value,
          }))}
          disabled={disabled}
          onSelect={(value) => onChange(option.id, { value })}
          selectedId={option.currentValue}
        />
      )}
      {pending ? (
        <span className="select-control__reason" role="status">
          Changing {option.name}…
        </span>
      ) : null}
      {error ? (
        <span className="select-control__reason" role="alert">
          {error}
        </span>
      ) : null}
    </label>
  );
}

import { memo } from "react";
import { Check } from "./leanIcons";
import type { SearchablePickerOption } from "./SearchablePicker";

export const PickerOption = memo(function PickerOption({
  active,
  candidate,
  id,
  onSelect,
  onActivate,
  onOptionElement,
  onRender,
  selected,
}: {
  active: boolean;
  candidate: SearchablePickerOption;
  id: string;
  onActivate: (id: string) => void;
  onOptionElement: (id: string, element: HTMLButtonElement | null) => void;
  onRender?: (id: string) => void;
  onSelect: (id: string) => void;
  selected: boolean;
}) {
  onRender?.(candidate.id);
  const disabled = Boolean(candidate.disabledReason);
  const logo =
    candidate.logo ??
    (candidate.icon ? `/harness-${candidate.icon}.svg` : null);
  return (
    <button
      ref={(element) => onOptionElement(candidate.id, element)}
      id={id}
      type="button"
      role="option"
      aria-label={candidate.accessibleLabel}
      title={candidate.disabledReason}
      className="searchable-picker__option menu-item"
      aria-disabled={disabled || undefined}
      aria-describedby={disabled ? `${id}-reason` : undefined}
      aria-selected={selected}
      data-option-id={candidate.id}
      data-active={active || undefined}
      disabled={disabled}
      tabIndex={-1}
      onMouseMove={() => !disabled && onActivate(candidate.id)}
      onClick={() => !disabled && onSelect(candidate.id)}
    >
      {logo ? (
        <img
          src={logo}
          alt=""
          aria-hidden="true"
          data-harness-icon={candidate.icon ?? "generic"}
        />
      ) : null}
      <span className="searchable-picker__option-copy">
        <span>{candidate.label}</span>
        {candidate.secondaryLabel ? (
          <small>{candidate.secondaryLabel}</small>
        ) : null}
      </span>
      {selected && logo ? (
        <Check
          className="searchable-picker__selected-check"
          size={14}
          aria-hidden="true"
        />
      ) : null}
      {candidate.disabledReason ? (
        <span id={`${id}-reason`} className="searchable-picker__reason">
          {candidate.disabledReason}
        </span>
      ) : null}
    </button>
  );
});

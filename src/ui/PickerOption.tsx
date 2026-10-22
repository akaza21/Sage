import { memo } from "react";
import { Check } from "./leanIcons";
import type { SearchablePickerOption } from "./SearchablePicker";

export const PickerOption = memo(function PickerOption({
  active,
  candidate,
  disabled = false,
  id,
  onSelect,
  onActivate,
  onOptionElement,
  onRender,
  selected,
  showSelectedCheck = false,
}: {
  active: boolean;
  candidate: SearchablePickerOption;
  disabled?: boolean;
  id: string;
  onActivate?: (id: string) => void;
  onOptionElement?: (id: string, element: HTMLButtonElement | null) => void;
  onRender?: (id: string) => void;
  onSelect: (id: string) => void;
  selected: boolean;
  showSelectedCheck?: boolean;
}) {
  onRender?.(candidate.id);
  const unavailable = disabled || Boolean(candidate.disabledReason);
  const logo =
    candidate.logo ??
    (candidate.icon ? `/harness-${candidate.icon}.svg` : null);
  return (
    <button
      ref={(element) => onOptionElement?.(candidate.id, element)}
      id={id}
      type="button"
      role="option"
      aria-label={candidate.accessibleLabel}
      title={candidate.disabledReason}
      className="searchable-picker__option menu-item"
      aria-disabled={unavailable || undefined}
      aria-describedby={candidate.disabledReason ? `${id}-reason` : undefined}
      aria-selected={selected}
      data-option-id={candidate.id}
      data-active={active || undefined}
      disabled={unavailable}
      tabIndex={-1}
      onMouseMove={() => !unavailable && onActivate?.(candidate.id)}
      onClick={() => !unavailable && onSelect(candidate.id)}
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
        {candidate.subtitle ? (
          <span className="searchable-picker__subtitle">
            {candidate.subtitle}
          </span>
        ) : null}
      </span>
      {selected && (logo || showSelectedCheck) ? (
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

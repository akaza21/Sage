import { memo } from "react";
import { Check } from "./leanIcons";
import { PickerLogo } from "./PickerLogo";
import type { SearchablePickerOption } from "./SearchablePicker";

export const PickerOption = memo(function PickerOption({
  active,
  tabIndex = -1,
  candidate,
  disabled = false,
  id,
  onSelect,
  onActivate,
  onRender,
  selected,
  showSelectedCheck = false,
}: {
  active: boolean;
  tabIndex?: 0 | -1;
  candidate: SearchablePickerOption;
  disabled?: boolean;
  id: string;
  onActivate?: (id: string) => void;
  onRender?: (id: string) => void;
  onSelect: (id: string) => void;
  selected: boolean;
  showSelectedCheck?: boolean;
}) {
  onRender?.(candidate.id);
  const unavailable = disabled || Boolean(candidate.disabledReason);
  const logo = candidate.logo ?? candidate.icon;
  const LeadingIcon = candidate.leadingIcon;
  return (
    <button
      id={id}
      type="button"
      role="option"
      aria-label={candidate.accessibleLabel}
      title={candidate.disabledReason}
      className={`searchable-picker__option menu-item${candidate.separatorBefore ? " searchable-picker__option--separated" : ""}`}
      aria-disabled={unavailable || undefined}
      aria-describedby={candidate.disabledReason ? `${id}-reason` : undefined}
      aria-selected={selected}
      data-option-id={candidate.id}
      data-active={active || undefined}
      disabled={unavailable}
      tabIndex={tabIndex}
      onMouseMove={() => !unavailable && onActivate?.(candidate.id)}
      onClick={() => !unavailable && onSelect(candidate.id)}
    >
      {logo ? (
        <PickerLogo logo={candidate.logo} icon={candidate.icon} />
      ) : LeadingIcon ? (
        <span className="searchable-picker__option-icon" aria-hidden="true">
          <LeadingIcon size={14} />
        </span>
      ) : null}
      <span className="searchable-picker__option-copy">
        <span>{candidate.label}</span>
        {candidate.subtitle ? (
          <span className="searchable-picker__subtitle">
            {candidate.subtitle}
          </span>
        ) : null}
        {candidate.disabledReason ? (
          <span id={`${id}-reason`} className="searchable-picker__reason">
            {candidate.disabledReason}
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
    </button>
  );
});

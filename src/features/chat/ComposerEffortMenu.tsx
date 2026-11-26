import { useState, type CSSProperties } from "react";
import type {
  AcpConfigOption,
  AcpConfigValue,
} from "../../domain/acpSessionConfig";
import { flattenChoices } from "../../domain/acpSessionConfigControls";
import { capitalizeLabel } from "../../domain/text";
import { Brain, RotateCcw } from "../../ui/leanIcons";
import "./composer-effort-menu.css";

/**
 * The thinking-level section of the composer's model popover: the level the
 * harness reports, a slider over the levels it lists, and a reset to its
 * default when it names one.
 */
export default function ComposerEffortMenu({
  option,
  disabled,
  pending,
  onSetOption,
}: {
  option: Extract<AcpConfigOption, { type: "select" }>;
  disabled: boolean;
  pending: boolean;
  onSetOption: (configId: string, value: AcpConfigValue) => Promise<unknown>;
}) {
  const [draft, setDraft] = useState<{
    option: typeof option;
    index: number;
  } | null>(null);
  const levels = flattenChoices(option);
  const index = levels.findIndex(
    (level) => level.value === option.currentValue,
  );
  const position =
    draft?.option === option && !disabled && !pending ? draft.index : index;
  const finishDrag = () => {
    if (draft?.option === option) select(draft.index);
    setDraft(null);
  };
  const defaultLevel = levels.find(
    (level) => level.value === option.defaultValue,
  );
  const select = (position: number) => {
    const level = levels[position];
    if (!disabled && !pending && level && level.value !== option.currentValue)
      void onSetOption(option.id, { value: level.value });
  };
  if (index < 0) return null;
  const label = capitalizeLabel(levels[position].name);
  return (
    <div className="composer-effort" role="group" aria-label="Thinking level">
      <div className="composer-effort__heading">
        <Brain size={13} aria-hidden="true" />
        <span className="composer-effort__level">{label}</span>
        {defaultLevel ? (
          <button
            type="button"
            aria-label="Reset effort"
            title="Reset effort"
            disabled={
              disabled || pending || defaultLevel.value === option.currentValue
            }
            onClick={(event) => {
              event.currentTarget
                .closest(".composer-effort")
                ?.querySelector("input")
                ?.focus();
              select(levels.indexOf(defaultLevel));
            }}
          >
            <RotateCcw size={13} aria-hidden="true" />
          </button>
        ) : (
          <span />
        )}
      </div>
      <div
        className="composer-effort__track"
        style={
          {
            "--effort-position": `${levels.length > 1 ? (position / (levels.length - 1)) * 100 : 0}%`,
          } as CSSProperties
        }
      >
        <input
          type="range"
          aria-label="Thinking level"
          aria-valuetext={label}
          aria-disabled={disabled || pending}
          min={0}
          max={levels.length - 1}
          step={1}
          value={position}
          onPointerDown={(event) => {
            if (disabled || pending) return;
            event.currentTarget.setPointerCapture?.(event.pointerId);
            setDraft({ option, index });
          }}
          onPointerUp={finishDrag}
          onPointerCancel={() => setDraft(null)}
          onBlur={finishDrag}
          onChange={(event) => {
            if (disabled || pending) return;
            const next = Number(event.currentTarget.value);
            if (draft) {
              if (draft.option === option) setDraft({ option, index: next });
            } else select(next);
          }}
          onKeyDown={(event) => {
            const position = {
              ArrowLeft: index - 1,
              ArrowDown: index - 1,
              ArrowRight: index + 1,
              ArrowUp: index + 1,
              Home: 0,
              End: levels.length - 1,
            }[event.key];
            if (position === undefined) return;
            event.preventDefault();
            event.stopPropagation();
            select(Math.max(0, Math.min(levels.length - 1, position)));
          }}
        />
        <div className="composer-effort__stops" aria-hidden="true">
          {levels.map((level) => (
            <span
              key={level.value}
              data-selected={level.value === levels[position].value}
            />
          ))}
        </div>
      </div>
    </div>
  );
}

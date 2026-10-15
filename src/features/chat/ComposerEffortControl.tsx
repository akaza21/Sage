import {
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type RefObject,
} from "react";
import type {
  AcpConfigOption,
  AcpConfigValue,
} from "../../domain/acpSessionConfig";
import { flattenChoices } from "../../domain/acpSessionConfigControls";
import { ChevronDown, RotateCcw, Sparkles } from "../../ui/leanIcons";
import { MenuPopover } from "../../ui/MenuPopover";

export function ComposerEffortControl({
  fallbackRef,
  option,
  modelLabel,
  open,
  onToggle,
  onDismiss,
  onOpenModel,
  modelAvailable,
  disabled,
  pending,
  onSetOption,
}: {
  fallbackRef: RefObject<HTMLButtonElement | null>;
  option: Extract<AcpConfigOption, { type: "select" }>;
  modelLabel: string;
  open: boolean;
  onToggle: () => void;
  onDismiss: () => void;
  onOpenModel: () => void;
  modelAvailable: boolean;
  disabled: boolean;
  pending: boolean;
  onSetOption: (configId: string, value: AcpConfigValue) => Promise<unknown>;
}) {
  const triggerRef = useRef<HTMLButtonElement>(null);
  const sliderRef = useRef<HTMLInputElement>(null);
  const panelId = useId();
  const [draft, setDraft] = useState<{
    option: typeof option;
    index: number;
  } | null>(null);
  if (!open && draft) setDraft(null);
  useLayoutEffect(() => {
    if (!open) return;
    const panel = document.getElementById(panelId);
    const restoreFocus = () => {
      queueMicrotask(() => {
        if (
          document.activeElement !== document.body &&
          !panel?.contains(document.activeElement)
        )
          return;
        const trigger = triggerRef.current;
        const fallback =
          trigger?.isConnected && !trigger.disabled
            ? trigger
            : fallbackRef.current;
        if (fallback && !fallback.disabled) fallback.focus();
        else
          fallback
            ?.closest("form")
            ?.querySelector<HTMLTextAreaElement>("textarea:not(:disabled)")
            ?.focus();
      });
    };
    if (disabled) restoreFocus();
    else queueMicrotask(() => sliderRef.current?.focus());
    return restoreFocus;
  }, [disabled, fallbackRef, open, panelId]);
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
  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        className="composer__control-trigger composer__effort-picker"
        title="Select effort"
        aria-controls={panelId}
        aria-haspopup="dialog"
        aria-expanded={open}
        disabled={disabled}
        onPointerDown={(event) => event.currentTarget.blur()}
        onClick={onToggle}
      >
        {levels[index].name} <ChevronDown size={12} aria-hidden="true" />
      </button>
      <MenuPopover
        id={panelId}
        ariaLabel="Thinking level"
        role="dialog"
        open={open}
        triggerRef={triggerRef}
        onDismiss={onDismiss}
        className="composer__popover composer__effort-menu"
      >
        <div className="composer-effort__heading">
          <Sparkles size={13} aria-hidden="true" />
          <div className="composer-effort__identity">
            <span className="composer-effort__level">{levels[index].name}</span>
            <button
              type="button"
              disabled={!modelAvailable}
              aria-label={`Select model: ${modelLabel}`}
              onClick={onOpenModel}
            >
              {modelLabel} <ChevronDown size={10} aria-hidden="true" />
            </button>
          </div>
          {defaultLevel ? (
            <button
              type="button"
              aria-label="Reset effort"
              title="Reset effort"
              disabled={
                disabled ||
                pending ||
                defaultLevel.value === option.currentValue
              }
              onClick={() => {
                sliderRef.current?.focus();
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
            ref={sliderRef}
            type="range"
            aria-label="Thinking level"
            aria-valuetext={levels[position].name}
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
      </MenuPopover>
    </>
  );
}

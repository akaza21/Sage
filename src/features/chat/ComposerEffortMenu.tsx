import {
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
import { capitalizeLabel } from "../../domain/text";
import { Brain, ChevronDown, RotateCcw } from "../../ui/leanIcons";
import { MenuPopover } from "../../ui/MenuPopover";

export default function ComposerEffortMenu({
  anchorRef,
  triggerRef,
  panelId,
  fallbackRef,
  option,
  modelLabel,
  open,
  onDismiss,
  onOpenModel,
  modelAvailable,
  disabled,
  pending,
  onSetOption,
}: {
  anchorRef?: RefObject<HTMLButtonElement | null>;
  triggerRef: RefObject<HTMLButtonElement | null>;
  panelId: string;
  fallbackRef: RefObject<HTMLButtonElement | null>;
  option: Extract<AcpConfigOption, { type: "select" }>;
  modelLabel: string;
  open: boolean;
  onDismiss: () => void;
  onOpenModel: () => void;
  modelAvailable: boolean;
  disabled: boolean;
  pending: boolean;
  onSetOption: (configId: string, value: AcpConfigValue) => Promise<unknown>;
}) {
  const panelTriggerRef = anchorRef ?? triggerRef;
  const sliderRef = useRef<HTMLInputElement>(null);
  const [draft, setDraft] = useState<{
    option: typeof option;
    index: number;
  } | null>(null);
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
        const trigger = panelTriggerRef.current;
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
  }, [disabled, fallbackRef, open, panelId, panelTriggerRef]);
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
  const label = levels[position].name;
  return (
    <>
      <MenuPopover
        id={panelId}
        ariaLabel="Thinking level"
        role="dialog"
        open={open}
        triggerRef={panelTriggerRef}
        onDismiss={() => {
          setDraft(null);
          onDismiss();
        }}
        className="composer__popover composer__effort-menu"
      >
        <div className="composer-effort__heading">
          <Brain size={13} aria-hidden="true" />
          <div className="composer-effort__identity">
            <span className="composer-effort__level">
              {capitalizeLabel(label)}
            </span>
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
            onBlur={() => setDraft(null)}
            onChange={(event) => {
              if (disabled || pending) return;
              const next = Number(event.currentTarget.value);
              if (draft) {
                if (draft.option === option) setDraft({ option, index: next });
              } else select(next);
            }}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                setDraft(null);
                return;
              }
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

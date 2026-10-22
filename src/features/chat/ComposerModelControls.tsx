import { useEffect, type RefObject } from "react";
import { ChevronDown, Sparkles } from "../../ui/leanIcons";
import { ComposerEffortControl } from "./ComposerEffortControl";
import { MenuPopover } from "../../ui/MenuPopover";
import type {
  AcpConfigOption,
  AcpConfigValue,
} from "../../domain/acpSessionConfig";
import { flattenChoices } from "../../domain/acpSessionConfigControls";
import {
  ChatModelPicker,
  MAX_VISIBLE_MODELS,
  type ComposerModelChoice,
} from "./ChatModelPicker";

export function ComposerModelControls({
  triggerRef,
  panelId,
  open,
  effortOpen,
  onToggle,
  onToggleEffort,
  effortAnchorRef,
  onDismiss,
  choices,
  selectedId,
  modelLabel,
  effortLabel,
  contextWindow,
  effortOption,
  disabled,
  pending,
  onSelectModel,
  onSetOption,
}: {
  triggerRef: RefObject<HTMLButtonElement | null>;
  panelId: string;
  open: boolean;
  effortOpen: boolean;
  onToggle: () => void;
  onToggleEffort: () => void;
  effortAnchorRef?: RefObject<HTMLButtonElement | null>;
  onDismiss: () => void;
  choices: readonly ComposerModelChoice[];
  selectedId: string | null;
  modelLabel: string;
  effortLabel: string | null;
  contextWindow: string | null;
  effortOption: AcpConfigOption | undefined;
  disabled: boolean;
  pending: boolean;
  onSelectModel: (modelId: string) => void;
  onSetOption: (configId: string, value: AcpConfigValue) => Promise<unknown>;
}) {
  const available = !disabled && !pending && choices.length > 0;
  const hasEffortControl =
    effortOption?.type === "select" &&
    flattenChoices(effortOption).some(
      (choice) => choice.value === effortOption.currentValue,
    );
  const modelDescription =
    modelLabel === "Model"
      ? undefined
      : `${modelLabel}${effortLabel ? ` ${effortLabel}` : ""}`;
  useEffect(() => {
    if (
      (open && !available) ||
      (effortOpen && (disabled || effortOption?.type !== "select"))
    )
      onDismiss();
  }, [available, disabled, effortOpen, effortOption?.type, onDismiss, open]);
  return (
    <>
      <div className="composer__model-group">
        <button
          ref={triggerRef}
          className="composer__control-trigger composer__picker-trigger composer__model-trigger"
          type="button"
          aria-label="Model"
          aria-description={modelDescription}
          title={modelDescription}
          aria-controls={panelId}
          aria-haspopup="listbox"
          aria-expanded={open && available}
          disabled={!available}
          onPointerDown={(event) => event.currentTarget.blur()}
          onClick={onToggle}
        >
          <Sparkles
            className="composer__model-icon"
            size={16}
            aria-hidden="true"
          />
          <span className="composer__model-label">{modelLabel}</span>
          {effortLabel && !hasEffortControl ? (
            <span className="composer__model-effort"> {effortLabel}</span>
          ) : null}
          <ChevronDown size={12} aria-hidden="true" />
        </button>
        {effortOption?.type === "select" ? (
          <ComposerEffortControl
            anchorRef={effortAnchorRef}
            fallbackRef={triggerRef}
            option={effortOption}
            modelLabel={modelLabel}
            open={effortOpen}
            onToggle={onToggleEffort}
            onDismiss={onDismiss}
            onOpenModel={onToggle}
            modelAvailable={available}
            disabled={disabled}
            pending={pending}
            onSetOption={onSetOption}
          />
        ) : null}
        {contextWindow ? (
          <span className="composer__capacity">{contextWindow} context</span>
        ) : null}
      </div>
      <MenuPopover
        open={open && available}
        onDismiss={onDismiss}
        restoreFocusOnTab
        triggerRef={triggerRef}
        initialFocus="selected"
        focusKey={choices
          .slice(0, MAX_VISIBLE_MODELS)
          .map((model) => model.id)
          .join("\0")}
        id={`${panelId}-popover`}
        role="none"
        className="composer__popover composer__model-menu composer-model-picker"
        ariaLabel="Select model"
      >
        <ChatModelPicker
          listboxId={panelId}
          models={choices}
          selectedId={selectedId}
          disabled={disabled || pending}
          onSelect={onSelectModel}
        />
      </MenuPopover>
    </>
  );
}

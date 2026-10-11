import { useEffect, type RefObject } from "react";
import { ChevronDown } from "../../ui/leanIcons";
import { SearchablePicker } from "../../ui/SearchablePicker";
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
  onToggle,
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
  onToggle: () => void;
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
  const modelDescription =
    modelLabel === "Model"
      ? undefined
      : `${modelLabel}${effortLabel ? ` ${effortLabel}` : ""}`;
  useEffect(() => {
    if (open && !available) onDismiss();
  }, [available, onDismiss, open]);
  const effortCandidates =
    effortOption?.type === "select"
      ? flattenChoices(effortOption).map((choice) => ({
          id: choice.value,
          label: choice.name || choice.value,
        }))
      : [];
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
          <span className="composer__model-label">{modelLabel}</span>
          {effortLabel ? (
            <span className="composer__model-effort"> {effortLabel}</span>
          ) : null}
          <ChevronDown size={12} aria-hidden="true" />
        </button>
        {effortOption?.type === "select" ? (
          <SearchablePicker
            ariaLabel="Thinking level"
            candidates={effortCandidates}
            className="composer__effort-picker"
            popoverClassName="composer__popover composer__effort-menu"
            triggerClassName="composer__control-trigger composer__picker-trigger"
            disabled={disabled || pending}
            onSelect={(value) => void onSetOption(effortOption.id, { value })}
            selectedId={effortOption.currentValue}
          />
        ) : null}
        {contextWindow ? (
          <span className="composer__capacity">{contextWindow} context</span>
        ) : null}
      </div>
      <MenuPopover
        open={open && available}
        onDismiss={(reason) => {
          if (reason === "tab") triggerRef.current?.focus();
          onDismiss();
        }}
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

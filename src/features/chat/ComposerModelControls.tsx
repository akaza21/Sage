import type { RefObject } from "react";
import { ChevronDown } from "../../ui/leanIcons";
import { SearchablePicker } from "../../ui/SearchablePicker";
import type {
  AcpConfigOption,
  AcpConfigValue,
} from "../../domain/acpSessionConfig";
import { flattenChoices } from "../../domain/acpSessionConfigControls";
import { ChatModelPicker, type ComposerModelChoice } from "./ChatModelPicker";

export function ComposerModelControls({
  triggerRef,
  panelRef,
  panelId,
  open,
  onToggle,
  choices,
  selectedId,
  modelLabel,
  harnessId,
  harnessName,
  contextWindow,
  effortOption,
  disabled,
  pending,
  onSelectModel,
  onSetOption,
}: {
  triggerRef: RefObject<HTMLButtonElement | null>;
  panelRef: RefObject<HTMLDivElement | null>;
  panelId: string;
  open: boolean;
  onToggle: () => void;
  choices: readonly ComposerModelChoice[];
  selectedId: string | null;
  modelLabel: string;
  harnessId: string;
  harnessName: string;
  contextWindow: string | null;
  effortOption: AcpConfigOption | undefined;
  disabled: boolean;
  pending: boolean;
  onSelectModel: (modelId: string) => void;
  onSetOption: (configId: string, value: AcpConfigValue) => Promise<unknown>;
}) {
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
          aria-controls={panelId}
          aria-haspopup="dialog"
          aria-expanded={open}
          disabled={disabled || choices.length === 0}
          onClick={onToggle}
        >
          {modelLabel}
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
      {open ? (
        <div
          ref={panelRef}
          id={panelId}
          role="dialog"
          tabIndex={-1}
          className="composer__popover composer__model-menu composer-model-picker"
          aria-label="Model picker"
        >
          <ChatModelPicker
            harnessId={harnessId}
            harnessName={harnessName}
            models={choices}
            selectedId={selectedId}
            disabled={disabled || pending}
            onSelect={onSelectModel}
          />
        </div>
      ) : null}
    </>
  );
}

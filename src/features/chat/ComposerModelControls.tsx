import {
  lazy,
  Suspense,
  useEffect,
  type RefObject,
  useId,
  useRef,
  type ComponentProps,
} from "react";
import { Brain, ChevronDown, Sparkles } from "../../ui/leanIcons";
import {
  type AcpConfigOption,
  type AcpConfigValue,
} from "../../domain/acpSessionConfig";
import { flattenChoices } from "../../domain/acpSessionConfigControls";
import { type ComposerModelChoice } from "./ChatModelPicker";
import { ComposerMenuLoading } from "./ComposerMenuLoading";
import type ComposerEffortMenu from "./ComposerEffortMenu";

const ModelMenu = lazy(() => import("./ComposerModelMenu"));

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
  awaitingSession = false,
  disabled,
  pending,
  readOnlyLabel,
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
  awaitingSession?: boolean;
  disabled: boolean;
  pending: boolean;
  readOnlyLabel?: boolean;
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
          data-readonly-label={(readOnlyLabel && !pending) || undefined}
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
        ) : awaitingSession ? (
          <button
            type="button"
            className="composer__control-trigger composer__effort-picker"
            aria-label="Thinking level"
            aria-description="Available when the harness advertises effort controls for this session."
            title="Available when the harness advertises effort controls for this session."
            disabled
          >
            <Brain
              className="composer__effort-icon"
              size={16}
              aria-hidden="true"
            />
            <span className="composer__effort-label">Thinking level</span>
          </button>
        ) : null}
        {contextWindow ? (
          <span className="composer__capacity">{contextWindow} context</span>
        ) : null}
      </div>
      {open && available ? (
        <Suspense
          fallback={
            <ComposerMenuLoading
              triggerRef={triggerRef}
              onDismiss={onDismiss}
            />
          }
        >
          <ModelMenu
            triggerRef={triggerRef}
            panelId={panelId}
            choices={choices}
            selectedId={selectedId}
            disabled={disabled || pending}
            onDismiss={onDismiss}
            onSelect={onSelectModel}
          />
        </Suspense>
      ) : null}
    </>
  );
}

const EffortMenu = lazy(() => import("./ComposerEffortMenu"));

type Props = Omit<
  ComponentProps<typeof ComposerEffortMenu>,
  "triggerRef" | "panelId"
> & { onToggle: () => void };

function ComposerEffortControl(props: Props) {
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelId = useId();
  const levels = flattenChoices(props.option);
  const index = levels.findIndex(
    (level) => level.value === props.option.currentValue,
  );
  if (index < 0) return null;
  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        className="composer__control-trigger composer__effort-picker"
        title="Select effort"
        aria-label={"Thinking level: " + levels[index].name}
        aria-controls={panelId}
        aria-haspopup="dialog"
        aria-expanded={props.open}
        disabled={props.disabled}
        onPointerDown={(event) => event.currentTarget.blur()}
        onClick={props.onToggle}
      >
        <Brain className="composer__effort-icon" size={14} aria-hidden="true" />
        <span className="composer__effort-label">{levels[index].name}</span>
        <ChevronDown size={12} aria-hidden="true" />
      </button>
      {props.open ? (
        <Suspense
          fallback={
            <ComposerMenuLoading
              triggerRef={props.anchorRef ?? triggerRef}
              onDismiss={props.onDismiss}
            />
          }
        >
          <EffortMenu {...props} triggerRef={triggerRef} panelId={panelId} />
        </Suspense>
      ) : null}
    </>
  );
}

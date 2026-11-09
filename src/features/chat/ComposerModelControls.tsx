import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  type RefObject,
} from "react";
import { Brain, ChevronDown, Sparkles } from "../../ui/leanIcons";
import {
  type AcpConfigOption,
  type AcpConfigValue,
} from "../../domain/acpSessionConfig";
import {
  flattenChoices,
  selectedConfigChoice,
} from "../../domain/acpSessionConfigControls";
import { type ComposerModelChoice } from "./ChatModelPicker";
import { ComposerMenuLoading } from "./ComposerMenuLoading";
import { ComposerEffortControl } from "./ComposerEffortControl";

const ModelMenu = lazy(() => import("./ComposerModelMenu"));

export function ComposerModelControls({
  triggerRef,
  panelId,
  open,
  effortOpen,
  effortSuspended = false,
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
  restored,
  notice,
  awaitingSession = false,
  disabled,
  effortDisabled = disabled,
  pending,
  readOnlyLabel,
  onSelectModel,
  onSetOption,
}: {
  triggerRef: RefObject<HTMLButtonElement | null>;
  panelId: string;
  open: boolean;
  effortOpen: boolean;
  effortSuspended?: boolean;
  onToggle: () => void;
  onToggleEffort: () => void;
  effortAnchorRef?: RefObject<HTMLButtonElement | null>;
  onDismiss: () => void;
  choices:
    | readonly ComposerModelChoice[]
    | Extract<AcpConfigOption, { type: "select" }>;
  selectedId: string | null;
  modelLabel: string;
  effortLabel: string | null;
  contextWindow: string | null;
  effortOption: AcpConfigOption | undefined;
  /** The note on values that are the chat's stored choices, not yet
   * reapplied to the session. */
  restored?: string;
  /** What the session could not take back of the stored choices. */
  notice?: string;
  awaitingSession?: boolean;
  disabled: boolean;
  effortDisabled?: boolean;
  pending: boolean;
  readOnlyLabel?: boolean;
  onSelectModel: (modelId: string) => void;
  onSetOption: (configId: string, value: AcpConfigValue) => Promise<unknown>;
}) {
  const count =
    "type" in choices ? flattenChoices(choices).length : choices.length;
  // The model is always a picker, so it reads as one control in every chat;
  // with a single advertised model its menu shows that model as selected.
  const available = !disabled && !pending && count > 0;
  // Callers pass a fresh handler on every render; a stable one lets the menu's
  // memoised options skip re-rendering on each focus or selection change.
  const latestSelect = useRef(onSelectModel);
  useLayoutEffect(() => {
    latestSelect.current = onSelectModel;
  });
  const selectModel = useCallback(
    (modelId: string) => latestSelect.current(modelId),
    [],
  );
  const hasEffortControl = !!selectedConfigChoice(effortOption);
  const modelDescription =
    modelLabel === "Model"
      ? undefined
      : [
          `${modelLabel}${effortLabel ? ` ${effortLabel}` : ""}`,
          restored,
          notice,
        ]
          .filter(Boolean)
          .join(". ");
  useEffect(() => {
    if (
      (open && !available) ||
      (effortOpen && (effortDisabled || !hasEffortControl))
    )
      onDismiss();
  }, [
    available,
    effortDisabled,
    effortOpen,
    hasEffortControl,
    onDismiss,
    open,
  ]);
  return (
    <>
      <div className="composer__model-group">
        <button
          ref={triggerRef}
          className="composer__control-trigger composer__picker-trigger composer__model-trigger"
          type="button"
          aria-label="Model"
          aria-description={modelDescription}
          data-selected-id={selectedId ?? undefined}
          title={modelDescription}
          data-readonly-label={(readOnlyLabel && !pending) || undefined}
          data-restored={restored ? true : undefined}
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
        {effortSuspended && !effortOpen ? (
          <span
            className="composer__control-trigger composer__effort-picker"
            data-overflow-control="effort"
            data-suspended
          />
        ) : effortOption?.type === "select" ? (
          <ComposerEffortControl
            triggerVisible={!effortSuspended}
            anchorRef={effortAnchorRef}
            fallbackRef={triggerRef}
            option={effortOption}
            restored={restored}
            modelLabel={modelLabel}
            open={effortOpen}
            onToggle={onToggleEffort}
            onDismiss={onDismiss}
            onOpenModel={onToggle}
            modelAvailable={available}
            disabled={effortDisabled}
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
        {notice ? (
          <span className="composer__config-notice" role="status">
            {notice}
          </span>
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
            onSelect={selectModel}
          />
        </Suspense>
      ) : null}
    </>
  );
}

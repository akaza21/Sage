import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  type RefObject,
} from "react";
import { ChevronDown, Sparkles } from "../../ui/leanIcons";
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
import type { DraftEffort } from "./DraftEffortControl";

const ModelMenu = lazy(() => import("./ComposerModelMenu"));
const DraftEffortControl = lazy(() => import("./DraftEffortControl"));

/**
 * One trigger shows the model and its thinking level. Its popover lists the
 * harness's models and, when the harness reports levels, the level slider.
 */
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
  draftEffort,
  restored,
  notice,
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
  onToggle: () => void;
  onDismiss: () => void;
  choices:
    | readonly ComposerModelChoice[]
    | Extract<AcpConfigOption, { type: "select" }>;
  selectedId: string | null;
  modelLabel: string;
  effortLabel: string | null;
  contextWindow: string | null;
  effortOption: AcpConfigOption | undefined;
  draftEffort?: DraftEffort;
  restored?: string;
  notice?: string;
  disabled: boolean;
  effortDisabled?: boolean;
  pending: boolean;
  readOnlyLabel?: boolean;
  onSelectModel: (modelId: string) => void;
  onSetOption: (configId: string, value: AcpConfigValue) => Promise<unknown>;
}) {
  const count =
    "type" in choices ? flattenChoices(choices).length : choices.length;
  const listed = !disabled && count > 0;
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
  const selectEffort = selectedConfigChoice(effortOption)
    ? (effortOption as Extract<AcpConfigOption, { type: "select" }>)
    : null;
  const draftLevels = !effortOption && draftEffort ? draftEffort : null;
  // The chip opens while either its models or its level can change, so a
  // level stays reachable without a catalog or while the prompt is blocked.
  // A change in flight keeps the popover open; only its controls wait.
  const openable =
    listed || (!!(selectEffort || draftLevels) && !effortDisabled);
  const effort = (mode: "label" | "panel") =>
    selectEffort ? (
      <ComposerEffortControl
        mode={mode}
        option={selectEffort}
        disabled={effortDisabled}
        pending={pending}
        onSetOption={onSetOption}
      />
    ) : draftLevels ? (
      <Suspense fallback={null}>
        <DraftEffortControl
          mode={mode}
          draft={draftLevels}
          disabled={effortDisabled}
          pending={pending}
        />
      </Suspense>
    ) : null;
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
    if (open && !openable) onDismiss();
  }, [openable, onDismiss, open]);
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
          aria-haspopup="dialog"
          aria-expanded={open && openable}
          disabled={!openable || pending}
          onPointerDown={(event) => event.currentTarget.blur()}
          onClick={onToggle}
        >
          <Sparkles
            className="composer__model-icon"
            size={16}
            aria-hidden="true"
          />
          <span className="composer__model-label">{modelLabel}</span>
          {selectEffort || draftLevels ? (
            effort("label")
          ) : effortLabel ? (
            <span className="composer__model-effort">{effortLabel}</span>
          ) : null}
          <ChevronDown size={12} aria-hidden="true" />
        </button>
        {notice ? (
          <span className="composer__config-notice" role="status">
            {notice}
          </span>
        ) : null}
      </div>
      {open && openable ? (
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
            contextWindow={contextWindow}
            disabled={disabled || pending}
            onDismiss={onDismiss}
            onSelect={selectModel}
          >
            {effort("panel")}
          </ModelMenu>
        </Suspense>
      ) : null}
    </>
  );
}

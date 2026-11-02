import { Plus, Settings } from "../../ui/leanIcons";
import { useHarnessKeyboardFocus } from "./useHarnessKeyboardFocus";

import { useLayoutEffect, useRef, useState } from "react";

import {
  cachedHarnessModels,
  harnessModelIdentity,
} from "../../app/harnessModelCache";
import type { HarnessDiscoveryPhase } from "../../app/harnessSlice";
import { useWorkbenchStore } from "../../app/store";
import type {
  AcpConfigValue,
  AcpSessionConfig,
} from "../../domain/acpSessionConfig";
import {
  flattenChoices,
  sessionConfigControls,
  sessionModeActions,
} from "../../domain/acpSessionConfigControls";
import { capitalizeLabel } from "../../domain/text";
import { useOverflowSteps } from "../../ui/useOverflowSteps";
import "../../styles/harness-picker.css";
import { HarnessSignInPicker } from "../harness/HarnessSignInPicker";
import { ComposerAccessControl } from "./ComposerAccessControl";
import { ComposerOptionsMenu } from "./ComposerAddActions";
import { composerOptionLabel } from "./composerChrome";
import { formatContextWindow } from "./composerModelContext";
import { ComposerModelControls } from "./ComposerModelControls";
import { useComposerModelName } from "./composerModelName";
import { useInheritedComposerModel } from "./inheritedComposerModel";
import { ProjectScopeChip } from "./ProjectScopeChip";
import {
  harnessPickerCandidates,
  useComposerPopovers,
} from "./useChatHarnessSelection";
import { useComposerModelChoices } from "./useComposerModelChoices";
import { useConfirmedModelLabel } from "./useConfirmedModelLabel";
/**
 * The toolbar's compaction steps, lowest priority first, as data-fit tokens
 * that chat-container-controls.css keys on. A disabled placeholder goes first
 * and the model name last: it truncates to a readable prefix before it gives
 * way to its icon.
 */
const TOOLBAR_STEPS = [
  "placeholder",
  "capacity",
  "access",
  "harness",
  "project",
  "effort",
  "effort-menu",
  "project-menu",
  "model-shrink",
  "model",
  "dense",
  "wrap",
] as const;

interface Props {
  accessBindingId?: string | null;
  imageAction?: { disabled: boolean; onChoose: () => void };
  onOpenGoal?: () => void;
  availableBinding: boolean;
  canPrompt: boolean;
  discoveryPhase: HarnessDiscoveryPhase;
  modelLabelScope: string;
  /** The Chat whose parent-inherited model and effort may label the composer. */
  inheritScope?: string;
  sessionHarnessId: string | null;
  sessionModelId: string | null;
  selectedHarnessId: string | null;
  selectedModelId: string | null;
  projectName: string | null;
  turnBusy: boolean;
  sessionConfig: AcpSessionConfig | null;
  modePending: boolean;
  modeError: string | null;
  focused: boolean;
  onSetSessionMode: (modeId: string) => Promise<unknown>;
  onSetSessionConfigOption: (
    configId: string,
    value: AcpConfigValue,
  ) => Promise<unknown>;
  onSelectHarness: (harnessId: string) => Promise<void>;
  onSelectModel: (modelId: string) => void;
  onModeRemoved: () => void;
}

export function ChatHarnessControls({
  accessBindingId,
  imageAction,
  onOpenGoal,
  availableBinding,
  canPrompt,
  modelLabelScope,
  inheritScope = "",
  sessionHarnessId,
  sessionModelId,
  selectedHarnessId,
  selectedModelId,
  projectName,
  turnBusy,
  sessionConfig,
  modePending,
  modeError,
  focused,
  onSetSessionMode,
  onSetSessionConfigOption,
  onSelectHarness,
  onSelectModel,
  onModeRemoved,
}: Props) {
  const harnesses = useWorkbenchStore((state) => state.harnesses);
  const projectId = useWorkbenchStore((state) => state.project?.id ?? null);
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const [effortFromAdd, setEffortFromAdd] = useState(false);
  const keyboardFocus = useHarnessKeyboardFocus();
  const {
    active,
    modelId,
    modelTrigger,
    optionsId,
    optionsTrigger: optionsTriggerRef,
    toggle,
    dismiss,
    setHarnessOpen,
  } = useComposerPopovers(turnBusy || projectTransitioning);
  const globalSelectedHarnessId = useWorkbenchStore(
    (state) => state.selectedHarnessId,
  );
  const globalModels = useWorkbenchStore((state) => state.models);
  const harnessesPending = harnesses.some(
    (harness) => harness.installState === "detecting",
  );
  const selectedHarness = harnesses.find(
    (harness) => harness.id === selectedHarnessId,
  );
  const accessHarness = accessBindingId
    ? harnesses.find((harness) => harness.bindingId === accessBindingId)
    : selectedHarness;
  const modelIdentity = selectedHarness?.accountModelDiscovery
    ? null
    : harnessModelIdentity(selectedHarness);
  const models =
    selectedHarnessId === globalSelectedHarnessId
      ? globalModels
      : (cachedHarnessModels(modelIdentity) ?? []);
  const selectedSessionConfig =
    sessionHarnessId === selectedHarnessId && sessionModelId === selectedModelId
      ? sessionConfig
      : null;
  const configOptions = sessionConfigControls(selectedSessionConfig);
  const modelOption = configOptions.find(
    (option) => (option.category ?? option.id).toLocaleLowerCase() === "model",
  );
  const effortOption =
    configOptions.find(
      (option) =>
        (option.category ?? option.id).toLocaleLowerCase() === "effort",
    ) ??
    configOptions.find(
      (option) =>
        (option.category ?? option.id).toLocaleLowerCase() === "thought_level",
    );
  const hasSessionModel = modelOption?.type === "select";
  const confirmedModelLabel = useConfirmedModelLabel(
    JSON.stringify([
      projectId,
      modelLabelScope,
      selectedHarnessId,
      selectedModelId,
      sessionModelId,
    ]),
    modelOption,
    models,
    selectedSessionConfig?.confirmedModel,
  );
  // A side chat or fork shows its parent's confirmed model and effort until
  // its own session confirms them; nothing else is ever invented.
  const inherited = useInheritedComposerModel(inheritScope);
  const inheriting =
    !confirmedModelLabel &&
    !selectedSessionConfig?.removed &&
    (!inherited?.id || inherited.id === selectedModelId)
      ? inherited
      : undefined;
  const modelLabel = useComposerModelName(
    confirmedModelLabel ?? inheriting?.model ?? "Model",
  );
  const confirmedEffort = selectedSessionConfig?.confirmedEffort;
  const effortLabel =
    composerOptionLabel(effortOption) ??
    (confirmedEffort
      ? capitalizeLabel(confirmedEffort)
      : (inheriting?.effort ?? null));
  const modelChoices = useComposerModelChoices(modelOption, models);
  const currentModelId = hasSessionModel
    ? modelOption.currentValue
    : (selectedSessionConfig?.confirmedModel?.id ?? selectedModelId);
  const contextWindow = formatContextWindow(
    models.find((model) => model.id === currentModelId)?.contextWindow,
  );
  const chooseModel = (modelId: string) => {
    if (hasSessionModel) {
      void onSetSessionConfigOption(modelOption.id, { value: modelId });
    } else {
      onSelectModel(modelId);
    }
    dismiss();
    modelTrigger.current?.focus();
  };
  const harnessCandidates = harnessPickerCandidates(harnesses).map(
    (candidate) => ({
      ...candidate,
      subtitle:
        candidate.id === selectedHarnessId && confirmedModelLabel
          ? `${confirmedModelLabel}${effortLabel ? ` ${effortLabel}` : ""}`
          : undefined,
    }),
  );
  const modeOptions = sessionModeActions(selectedSessionConfig);
  const modeCandidates = modeOptions.map((mode) => ({
    id: mode.id,
    label: mode.name || mode.id,
  }));
  const otherOptions = sessionConfigControls(selectedSessionConfig, "other");
  const canSelectEffort =
    canPrompt &&
    effortOption?.type === "select" &&
    flattenChoices(effortOption).some(
      (choice) => choice.value === effortOption.currentValue,
    );
  // Effort and the project chip move into the options menu only when there is
  // a menu item to take their place.
  const steps = TOOLBAR_STEPS.filter(
    (step) =>
      (step !== "effort-menu" || canSelectEffort) &&
      (step !== "project-menu" || Boolean(projectName)),
  );
  const controls = useRef<HTMLDivElement>(null);
  const taken = steps.slice(
    0,
    useOverflowSteps(controls, steps, active !== null),
  );
  const effortInMenu = taken.includes("effort-menu");
  const projectInMenu = taken.includes("project-menu");
  // The options trigger appears once a control moves into its menu; that
  // refits the row with the trigger in place, so the controls only return to
  // the toolbar once they fit beside it.
  const hasActions = Boolean(
    imageAction ||
    onOpenGoal ||
    effortInMenu ||
    projectInMenu ||
    modeOptions.length > 0 ||
    otherOptions.some((option) => option.type !== "unsupported"),
  );
  const actionsKey = JSON.stringify([
    selectedHarnessId,
    selectedModelId,
    selectedSessionConfig?.sageAgentSessionId,
    Boolean(imageAction),
    Boolean(onOpenGoal),
    canSelectEffort,
    effortInMenu,
    projectInMenu,
    modeCandidates.map((mode) => mode.id),
    otherOptions.map((option) => [
      option.id,
      option.type,
      flattenChoices(option).map((choice) => choice.value),
    ]),
  ]);
  const previousActionsKey = useRef(actionsKey);
  useLayoutEffect(() => {
    if (previousActionsKey.current !== actionsKey && active === "options") {
      const element = document.activeElement;
      const menuHadFocus =
        element === document.body ||
        Boolean(
          element && document.getElementById(optionsId)?.contains(element),
        );
      dismiss();
      if (focused && menuHadFocus) onModeRemoved();
    }
    previousActionsKey.current = actionsKey;
  }, [actionsKey, active, dismiss, focused, onModeRemoved, optionsId]);
  const closeActions = () => {
    dismiss();
    optionsTriggerRef.current?.focus();
  };
  return (
    <div ref={controls} className="composer__controls">
      {hasActions ? (
        <button
          ref={optionsTriggerRef}
          className="composer__plus"
          type="button"
          aria-label="Composer options"
          aria-controls={optionsId}
          aria-haspopup="menu"
          aria-expanded={active === "options"}
          disabled={turnBusy || projectTransitioning}
          onClick={(event) => {
            optionsTriggerRef.current = event.currentTarget;
            toggle("options");
          }}
        >
          <Plus size={16} aria-hidden="true" />
        </button>
      ) : null}
      {availableBinding && accessHarness?.bindingId ? (
        <ComposerAccessControl
          bindingId={accessHarness.bindingId}
          capabilities={accessHarness.accessModes}
          disabled={turnBusy || projectTransitioning}
          open={active === "access"}
          onToggle={() => toggle("access")}
          onDismiss={dismiss}
        />
      ) : null}
      <label
        className="composer__control-trigger composer__harness"
        data-keyboard-focus={keyboardFocus}
      >
        <HarnessSignInPicker
          harnesses={harnesses}
          ariaLabel="Harness"
          open={active === "harness"}
          onOpenChange={setHarnessOpen}
          triggerIcon={<Settings size={16} aria-hidden="true" />}
          candidates={harnessCandidates}
          popoverClassName="composer__popover composer__harness-menu"
          popoverGap={8}
          triggerClassName="composer__control-trigger composer__picker-trigger"
          sectionLabel="Harnesses"
          searchable={false}
          disabled={projectTransitioning || turnBusy}
          state={harnessesPending ? "loading" : undefined}
          stateTitle={harnessesPending ? "Detecting harnesses…" : undefined}
          onSelect={(id) => void onSelectHarness(id)}
          selectedId={selectedHarnessId}
        />
      </label>
      <ComposerModelControls
        triggerRef={modelTrigger}
        panelId={modelId}
        open={active === "model"}
        effortOpen={active === "effort"}
        onToggle={() => toggle("model")}
        onToggleEffort={() => {
          setEffortFromAdd(false);
          toggle("effort");
        }}
        effortAnchorRef={effortFromAdd ? optionsTriggerRef : undefined}
        onDismiss={dismiss}
        choices={modelChoices}
        selectedId={currentModelId}
        modelLabel={modelLabel}
        effortLabel={confirmedModelLabel || inheriting ? effortLabel : null}
        contextWindow={contextWindow}
        effortOption={effortOption}
        awaitingSession={
          sessionHarnessId === null &&
          selectedHarnessId !== null &&
          availableBinding
        }
        disabled={turnBusy || projectTransitioning || !canPrompt}
        pending={modePending}
        readOnlyLabel={
          !projectTransitioning &&
          (!!inheriting || (turnBusy && modelChoices.length > 0))
        }
        onSelectModel={chooseModel}
        onSetOption={onSetSessionConfigOption}
      />
      {projectName ? (
        <ProjectScopeChip
          projectName={projectName}
          disabled={projectTransitioning || turnBusy}
          open={active === "project"}
          onOpenChange={(open) => {
            if (open && active !== "project") toggle("project");
            else if (!open && active === "project") dismiss();
          }}
        />
      ) : null}
      {active === "options" && hasActions ? (
        <ComposerOptionsMenu
          id={optionsId}
          triggerRef={optionsTriggerRef}
          onDismiss={dismiss}
          projectAction={
            projectInMenu && projectName ? (
              <ProjectScopeChip projectName={projectName} menuItem />
            ) : undefined
          }
          sessionConfig={selectedSessionConfig}
          pending={modePending}
          disabled={turnBusy || projectTransitioning}
          imageAction={imageAction}
          onOpenGoal={onOpenGoal}
          onOpenEffort={
            canSelectEffort && effortInMenu
              ? () => {
                  setEffortFromAdd(true);
                  toggle("effort");
                }
              : undefined
          }
          onClose={closeActions}
          onSetSessionMode={onSetSessionMode}
          onSetSessionConfigOption={onSetSessionConfigOption}
        />
      ) : null}
      {modePending ? (
        <span className="select-control__reason" role="status">
          Changing session settings…
        </span>
      ) : null}
      {modeError ? (
        <span className="select-control__reason" role="alert" title={modeError}>
          {modeError}
        </span>
      ) : null}
    </div>
  );
}

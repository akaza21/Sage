import { IconButton } from "../../ui/IconButton";
import { useHarnessKeyboardFocus } from "./useHarnessKeyboardFocus";
import { Plus, Settings } from "../../ui/leanIcons";

import { useState } from "react";
import type { ChatUsage } from "./acp/chatUsage";
import { ComposerContext } from "./ComposerContext";

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
  selectedConfigChoice,
  sessionConfigControls,
  sessionModeActions,
} from "../../domain/acpSessionConfigControls";
import { capitalizeLabel } from "../../domain/text";
import "../../styles/harness-picker.css";
import { HarnessSignInPicker } from "../harness/HarnessSignInPicker";
import { ComposerAccessControl } from "./ComposerAccessControl";
import { composerOptionLabel } from "./composerChrome";
import { useComposerToolbarFit } from "./useComposerToolbarFit";
import { formatContextWindow } from "./composerModelContext";
import { ComposerOptionsPart } from "./lazyParts";
import { ComposerMenuLoading } from "./ComposerMenuLoading";
import { ComposerModelControls } from "./ComposerModelControls";
import { composerModelName } from "./composerModelName";
import { useInheritedComposerModel } from "./inheritedComposerModelState";
import { ProjectScopeChip } from "./ProjectScopeChip";
import {
  harnessPickerCandidates,
  useComposerPopovers,
} from "./useChatHarnessSelection";
import { useComposerModelSelection } from "./useComposerModelChoices";
import { useConfirmedModelLabel } from "./useConfirmedModelLabel";

interface Props {
  contextUsage?: ChatUsage | null;
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
  contextUsage,
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
  // Effort levels are known only once a session runs the selected harness and
  // model; until then effort shows as unavailable, however the selection and
  // the session were reached.
  // A restored config is passed only for the session the selection continues.
  const sessionOwnsSelection =
    !!sessionConfig?.restored ||
    (sessionHarnessId === selectedHarnessId &&
      sessionModelId === selectedModelId);
  const selectedSessionConfig = sessionOwnsSelection ? sessionConfig : null;
  const configOptions = sessionConfigControls(selectedSessionConfig);
  const findOption = (category: string) =>
    configOptions.find(
      (option) =>
        (option.category ?? option.id).toLocaleLowerCase() === category,
    );
  const modelOption = findOption("model");
  const effortOption = findOption("effort") ?? findOption("thought_level");
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
  const modelLabel = composerModelName(
    confirmedModelLabel ?? inheriting?.model ?? "Model",
  );
  const confirmedEffort = selectedSessionConfig?.confirmedEffort;
  const effortName =
    composerOptionLabel(effortOption) ?? confirmedEffort ?? inheriting?.effort;
  const effortLabel = effortName ? capitalizeLabel(effortName) : null;
  const {
    choices: modelChoices,
    currentId: currentModelId,
    unchangedId: unchangedModelId,
  } = useComposerModelSelection(
    modelOption,
    models,
    selectedSessionConfig,
    confirmedModelLabel,
    selectedModelId,
  );
  const contextWindow = formatContextWindow(
    models.find((model) => model.id === currentModelId)?.contextWindow,
  );
  const chooseModel = (modelId: string) => {
    const changed = modelId !== unchangedModelId;
    if (changed && modelOption?.type === "select")
      void onSetSessionConfigOption(modelOption.id, { value: modelId });
    else if (changed) onSelectModel(modelId);
    dismiss();
    modelTrigger.current?.focus();
  };
  const harnessCandidates = harnessPickerCandidates(
    harnesses,
    selectedHarnessId,
    confirmedModelLabel
      ? `${confirmedModelLabel}${effortLabel ? ` ${effortLabel}` : ""}`
      : undefined,
  );
  const modeOptions = sessionModeActions(selectedSessionConfig);
  const otherOptions = sessionConfigControls(selectedSessionConfig, "other");
  const canSelectEffort = !!selectedConfigChoice(effortOption);
  const { controls, effortInMenu, projectInMenu, contextVisible } =
    useComposerToolbarFit(
      canSelectEffort,
      Boolean(projectName),
      active !== null && active !== "options",
    );
  // "+" is always the row's first control, as in Codex, so the toolbar keeps
  // the same shape in every chat; it is disabled while it has nothing to offer.
  const hasActions = Boolean(
    imageAction ||
    onOpenGoal ||
    effortInMenu ||
    projectInMenu ||
    modeOptions.length > 0 ||
    otherOptions.some((option) => option.type !== "unsupported"),
  );
  const closeActions = () => {
    dismiss();
    optionsTriggerRef.current?.focus();
  };
  return (
    <>
      <div
        ref={controls}
        className="composer__controls"
        onKeyDown={(event) => {
          if (event.key === "Escape" && active === "options") {
            event.preventDefault();
            dismiss();
          }
        }}
      >
        <IconButton
          ref={optionsTriggerRef}
          className="composer__plus"
          aria-label="Composer options"
          title={hasActions ? undefined : "No options for this harness"}
          aria-controls={optionsId}
          aria-haspopup="menu"
          aria-expanded={active === "options"}
          disabled={
            turnBusy ||
            projectTransitioning ||
            (!hasActions && active !== "options")
          }
          onClick={() => toggle("options")}
        >
          <Plus size={16} aria-hidden="true" />
        </IconButton>
        {availableBinding && accessHarness?.bindingId ? (
          <ComposerAccessControl
            bindingId={accessHarness.bindingId}
            capabilities={accessHarness.accessModes}
            sessionConfig={sessionConfig}
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
            popoverPlacement="above-start"
            triggerClassName="composer__control-trigger composer__picker-trigger"
            sectionLabel="Harnesses"
            searchable={false}
            disabled={projectTransitioning || turnBusy}
            state={harnessesPending ? "loading" : undefined}
            stateTitle={harnessesPending ? "Detecting harnesses…" : undefined}
            onSelect={onSelectHarness}
            selectedId={selectedHarnessId}
          />
        </label>
        <ComposerModelControls
          triggerRef={modelTrigger}
          panelId={modelId}
          open={active === "model"}
          effortOpen={active === "effort"}
          effortSuspended={effortInMenu}
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
          restored={selectedSessionConfig?.restored}
          notice={selectedSessionConfig?.notice}
          awaitingSession={
            !sessionOwnsSelection &&
            selectedHarnessId !== null &&
            availableBinding
          }
          disabled={turnBusy || projectTransitioning || !canPrompt}
          effortDisabled={turnBusy || projectTransitioning}
          pending={modePending}
          readOnlyLabel={
            !projectTransitioning &&
            (!!inheriting ||
              (turnBusy && (hasSessionModel || models.length > 0)))
          }
          onSelectModel={chooseModel}
          onSetOption={onSetSessionConfigOption}
        />
        {projectName && projectInMenu ? (
          <span
            className="project-scope"
            data-overflow-control="project"
            data-suspended
          />
        ) : projectName ? (
          <ProjectScopeChip
            projectName={projectName}
            disabled={projectTransitioning || turnBusy}
            popoverPlacement="above-end"
            open={active === "project"}
            onOpenChange={(open) => {
              if (open && active !== "project") toggle("project");
              else if (!open && active === "project") dismiss();
            }}
          />
        ) : null}
        {active === "options" ? (
          <ComposerOptionsPart
            part="ComposerOptionsMenu"
            accessModes={accessHarness?.accessModes}
            fallback={
              <ComposerMenuLoading
                triggerRef={optionsTriggerRef}
                onDismiss={dismiss}
              />
            }
            id={optionsId}
            selectionKey={JSON.stringify([selectedHarnessId, selectedModelId])}
            canSelectEffort={canSelectEffort}
            focused={focused}
            onModeRemoved={onModeRemoved}
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
              effortInMenu
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
          <span
            className="select-control__reason"
            role="alert"
            title={modeError}
          >
            {modeError}
          </span>
        ) : null}
      </div>
      {contextUsage && (
        <ComposerContext usage={contextUsage} visible={contextVisible} />
      )}
    </>
  );
}

import type { ChatHarnessControlsProps } from "./chatHarnessControlsProps";
import { IconButton } from "../../ui/IconButton";
import { Plus } from "../../ui/leanIcons";

import { useState } from "react";
import { ComposerContext } from "./ComposerContext";

import {
  cachedHarnessModels,
  harnessModelIdentity,
} from "../../app/harnessModelCache";
import { useWorkbenchStore } from "../../app/store";
import {
  selectedConfigChoice,
  sessionConfigControls,
  sessionModeActions,
} from "../../domain/acpSessionConfigControls";
import { capitalizeLabel } from "../../domain/text";
import { ComposerAccessControl } from "./ComposerAccessControl";
import { ComposerHarnessPicker } from "./ComposerHarnessPicker";
import { composerOptionLabel } from "./composerChrome";
import { useComposerToolbarFit } from "./useComposerToolbarFit";
import { formatContextWindow } from "./composerModelContext";
import { ComposerOptionsPart } from "./lazyParts";
import { ComposerMenuLoading } from "./ComposerMenuLoading";
import { ComposerModelControls } from "./ComposerModelControls";
import type { DraftEffort } from "./DraftEffortControl";
import { composerModelName } from "./composerModelName";
import { useInheritedComposerModel } from "./inheritedComposerModelState";
import { ProjectScopeChip } from "./ProjectScopeChip";
import {
  harnessPickerCandidates,
  useComposerPopovers,
} from "./useChatHarnessSelection";
import { useComposerModelSelection } from "./useComposerModelChoices";
import { useConfirmedModelLabel } from "./useConfirmedModelLabel";

export function ChatHarnessControls({
  draftPickers,
  contextUsage,
  usageRequest,
  accessTarget,
  addActions = {},
  attachSlot,
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
}: ChatHarnessControlsProps) {
  const harnesses = useWorkbenchStore((state) => state.harnesses);
  const projectId = useWorkbenchStore((state) => state.project?.id ?? null);
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const [effortFromAdd, setEffortFromAdd] = useState(false);
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
  const selectedHarness = harnesses.find(
    (harness) => harness.id === selectedHarnessId,
  );
  const accessBindingId = accessTarget?.bindingId;
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
  const currentModel = models.find((model) => model.id === currentModelId);
  const modelLabel = composerModelName(
    confirmedModelLabel ??
      inheriting?.model ??
      (draftPickers ? currentModel?.name : null) ??
      "Model",
  );
  // Before the chat's first session (`draftChatId` is set only then), the
  // levels the harness lists for the selected model can be chosen.
  const draft = {
    chatId: accessTarget?.draftChatId,
    bindingId: selectedHarness?.bindingId,
    modelId: currentModelId,
    levels: effortOption ? undefined : currentModel?.effortLevels,
  };
  const draftEffort =
    (draft.chatId || draftPickers) &&
    draft.bindingId &&
    draft.modelId &&
    draft.levels?.length
      ? ({
          ...draft,
          value: draftPickers?.effort,
          onChange: draftPickers?.onEffortChange,
        } as DraftEffort)
      : undefined;

  const contextWindow = formatContextWindow(currentModel?.contextWindow);
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
  const canSelectEffort = !!selectedConfigChoice(effortOption) || !!draftEffort;
  const { controls, effortInMenu, projectInMenu, contextVisible } =
    useComposerToolbarFit(
      !draftPickers && canSelectEffort,
      !draftPickers && Boolean(projectName),
      active !== null && active !== "options",
    );
  // "+" is always the row's first control, as in Codex, so the toolbar keeps
  // the same shape in every chat; it is disabled while it has nothing to offer.
  const hasActions = Boolean(
    Object.values(addActions).some(Boolean) ||
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
        {!draftPickers ? (
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
        ) : null}
        {attachSlot}
        {availableBinding && accessHarness?.bindingId ? (
          <ComposerAccessControl
            bindingId={accessHarness.bindingId}
            chatId={accessTarget?.draftChatId}
            value={draftPickers?.access}
            onChange={draftPickers?.onAccessChange}
            capabilities={accessHarness.accessModes}
            sessionConfig={sessionConfig}
            disabled={turnBusy || projectTransitioning}
            open={active === "access"}
            onToggle={() => toggle("access")}
            onDismiss={dismiss}
          />
        ) : null}
        <ComposerHarnessPicker
          open={active === "harness"}
          onOpenChange={setHarnessOpen}
          candidates={harnessCandidates}
          disabled={projectTransitioning || turnBusy}
          onSelect={onSelectHarness}
          selectedId={selectedHarnessId}
        />
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
          draftEffort={draftEffort}
          restored={selectedSessionConfig?.restored}
          notice={selectedSessionConfig?.notice}
          awaitingSession={
            !draftPickers &&
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
            {...addActions}
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
      <ComposerContext
        usage={contextUsage ?? null}
        usageRequest={usageRequest}
        visible={contextVisible}
      />
    </>
  );
}

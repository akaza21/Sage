import type { ChatHarnessControlsProps } from "./chatHarnessControlsProps";
import { IconButton } from "../../ui/IconButton";
import { Plus } from "../../ui/leanIcons";

import { useReducer, useState } from "react";
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
import { usePlanAccess } from "./usePlanAccess";
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
  accessTarget: { bindingId: accessBindingId, draftChatId, endedPage } = {},
  addActions = {},
  optionsTriggerId,
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
  const hasProjects = useWorkbenchStore((state) => state.projects.length > 0);
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
  } = useComposerPopovers(projectTransitioning, turnBusy);
  const globalSelectedHarnessId = useWorkbenchStore(
    (state) => state.selectedHarnessId,
  );
  const globalModels = useWorkbenchStore((state) => state.models);
  const selectedHarness = harnesses.find(
    (harness) => harness.id === selectedHarnessId,
  );
  const accessHarness = accessBindingId
    ? harnesses.find((harness) => harness.bindingId === accessBindingId)
    : selectedHarness;
  const { planAccess, confirmation } = usePlanAccess({
    harness: accessHarness,
    available: availableBinding,
    draftChatId,
    endedPage,
    sessionConfig,
    accessOpen: active === "access",
    openAccess: () => toggle("access"),
  });
  const [, listed] = useReducer((count: number) => count + 1, 0);
  const knownModels =
    selectedHarnessId === globalSelectedHarnessId
      ? globalModels
      : cachedHarnessModels(harnessModelIdentity(selectedHarness));
  const models = knownModels ?? [];
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
  // While the next send starts a session (`draftChatId` is set only then),
  // the levels the harness lists for the selected model can be chosen.
  const draft = {
    chatId: draftChatId,
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
  const { controls, effortInMenu, contextVisible } = useComposerToolbarFit(
    !draftPickers && canSelectEffort,
    active !== null && active !== "options",
  );
  const projectAction =
    projectName && hasProjects ? (
      <ProjectScopeChip
        projectName={projectName}
        menuItem
        disabled={turnBusy}
      />
    ) : undefined;
  // "+" is always the row's first control, as in Codex, so the toolbar keeps
  // the same shape in every chat; it is disabled while it has nothing to offer.
  const hasActions = Boolean(
    Object.values(addActions).some(Boolean) ||
    planAccess ||
    effortInMenu ||
    projectAction ||
    modeOptions.length > 0 ||
    otherOptions.some((option) => option.type !== "unsupported"),
  );
  const closeActions = () => {
    dismiss();
    optionsTriggerRef.current?.focus();
  };
  return (
    <>
      {knownModels || !selectedHarness ? null : (
        <ComposerOptionsPart
          part="ListedHarnessModels"
          harness={selectedHarness}
          onListed={listed}
        />
      )}
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
            id={optionsTriggerId}
            className="composer__plus"
            aria-label="Composer options"
            title={hasActions ? undefined : "No options for this harness"}
            aria-controls={optionsId}
            aria-haspopup="menu"
            aria-expanded={active === "options"}
            disabled={
              projectTransitioning || (!hasActions && active !== "options")
            }
            onClick={() => toggle("options")}
          >
            <Plus size={16} aria-hidden="true" />
          </IconButton>
        ) : null}
        {availableBinding && accessHarness?.bindingId ? (
          <ComposerAccessControl
            bindingId={accessHarness.bindingId}
            harnessName={accessHarness.name}
            chatId={draftChatId}
            endedPage={endedPage}
            value={draftPickers?.access}
            onChange={draftPickers?.onAccessChange}
            confirmation={confirmation}
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
            projectAction={projectAction}
            planAccess={planAccess}
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

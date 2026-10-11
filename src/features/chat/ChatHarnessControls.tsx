import { Plus, Settings, ShieldCheck } from "../../ui/leanIcons";

import {
  useLayoutEffect,
  useMemo,
  useRef,
  type ReactNode,
  type RefObject,
} from "react";

import { useWorkbenchStore } from "../../app/store";
import {
  cachedHarnessModels,
  harnessModelIdentity,
} from "../../app/harnessModelCache";
import type { HarnessDiscoveryPhase } from "../../app/harnessSlice";
import { useSessionAccessModeStore } from "../../app/sessionAccessModeStore";
import {
  SESSION_ACCESS_MODE_SCOPE,
  sessionAccessModeAvailability,
  sessionAccessModeCandidates,
  sessionAccessModeCopy,
} from "../../domain/sessionAccessMode";
import type { SessionAccessMode } from "../../domain/sessionAccessMode";
import type {
  AcpConfigValue,
  AcpSessionConfig,
} from "../../domain/acpSessionConfig";
import {
  flattenChoices,
  sessionConfigControls,
} from "../../domain/acpSessionConfigControls";
import { SearchablePicker } from "../../ui/SearchablePicker";
import { MenuPopover } from "../../ui/MenuPopover";
import { formatContextWindow } from "./composerModelContext";
import { ComposerModelControls } from "./ComposerModelControls";
import { ChatSessionConfigControls } from "./ChatSessionConfigControls";
import { ProjectScopeChip } from "./ProjectScopeChip";
import { composerOptionLabel } from "./composerChrome";
import { useConfirmedModelLabel } from "./useConfirmedModelLabel";
import {
  harnessPickerCandidates,
  useComposerPopovers,
} from "./useChatHarnessSelection";
import "../../styles/harness-picker.css";
interface Props {
  imageControl?: ReactNode;
  availableBinding: boolean;
  canPrompt: boolean;
  discoveryPhase: HarnessDiscoveryPhase;
  modelLabelScope: string;
  sessionHarnessId: string | null;
  sessionModelId: string | null;
  selectedHarnessId: string | null;
  selectedModelId: string | null;
  projectName: string | null;
  turnBusy: boolean;
  sessionConfig: AcpSessionConfig | null;
  permissionRequests: boolean;
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
  modeControlRef?: RefObject<HTMLLabelElement | null>;
}

export function ChatHarnessControls({
  imageControl,
  availableBinding,
  canPrompt,
  modelLabelScope,
  sessionHarnessId,
  sessionModelId,
  selectedHarnessId,
  selectedModelId,
  projectName,
  turnBusy,
  sessionConfig,
  permissionRequests,
  modePending,
  modeError,
  focused,
  onSetSessionMode,
  onSetSessionConfigOption,
  onSelectHarness,
  onSelectModel,
  onModeRemoved,
  modeControlRef: externalModeControlRef,
}: Props) {
  const harnesses = useWorkbenchStore((state) => state.harnesses);
  const projectId = useWorkbenchStore((state) => state.project?.id ?? null);
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const {
    active,
    modelId,
    modelTrigger,
    optionsId,
    optionsTrigger: optionsTriggerRef,
    toggle,
    dismiss,
  } = useComposerPopovers(turnBusy || projectTransitioning);
  const globalSelectedHarnessId = useWorkbenchStore(
    (state) => state.selectedHarnessId,
  );
  const globalModels = useWorkbenchStore((state) => state.models);
  const accessMode = useSessionAccessModeStore((state) => state.mode);
  const effectiveAccessMode = sessionAccessModeAvailability(
    accessMode,
    permissionRequests,
  ).effective;
  const accessModeReady = useSessionAccessModeStore((state) => state.ready);
  const accessModeError = useSessionAccessModeStore((state) => state.error);
  const setAccessMode = useSessionAccessModeStore((state) => state.setMode);
  const accessModeCandidates = useMemo(
    () => sessionAccessModeCandidates(permissionRequests),
    [permissionRequests],
  );
  const harnessesPending = harnesses.some(
    (harness) => harness.installState === "detecting",
  );
  const selectedHarness = harnesses.find(
    (harness) => harness.id === selectedHarnessId,
  );
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
  const effortOption = configOptions.find(
    (option) => (option.category ?? option.id).toLocaleLowerCase() === "effort",
  );
  const hasSessionModel = modelOption?.type === "select";
  const modelLabel = useConfirmedModelLabel(
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
  const confirmedEffort = selectedSessionConfig?.confirmedEffort;
  const effortLabel =
    composerOptionLabel(effortOption) ??
    (confirmedEffort
      ? confirmedEffort[0].toLocaleUpperCase() + confirmedEffort.slice(1)
      : null);
  const modelChoices = hasSessionModel
    ? flattenChoices(modelOption).map((choice) => ({
        id: choice.value,
        name: choice.name || choice.value,
      }))
    : models;
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
  const harnessCandidates = harnessPickerCandidates(harnesses);
  const internalModeControlRef = useRef<HTMLLabelElement>(null);
  const modeControlRef = externalModeControlRef ?? internalModeControlRef;
  const modeOptions = selectedSessionConfig?.modes?.availableModes ?? [];
  const hasModes = !selectedSessionConfig?.removed && modeOptions.length > 0;
  const modeCandidates = modeOptions.map((mode) => ({
    id: mode.id,
    label: mode.name || mode.id,
  }));
  const hadModes = useRef(hasModes);
  useLayoutEffect(() => {
    const activeElement = document.activeElement;
    const modePopoverHadFocus =
      activeElement instanceof HTMLElement &&
      activeElement.getAttribute("role") === "combobox" &&
      activeElement.getAttribute("aria-label") === "Session mode";
    const modeControlHadFocus = Boolean(
      activeElement === document.body ||
      modePopoverHadFocus ||
      (activeElement instanceof HTMLElement &&
        (!activeElement.isConnected ||
          modeControlRef.current?.contains(activeElement))),
    );
    if (hadModes.current && !hasModes && focused && modeControlHadFocus) {
      onModeRemoved();
    }
    hadModes.current = hasModes;
  }, [focused, hasModes, modeControlRef, onModeRemoved]);
  return (
    <div className="composer__controls">
      <button
        className="composer__plus"
        type="button"
        aria-label="Composer options"
        aria-controls={optionsId}
        aria-haspopup="dialog"
        aria-expanded={active === "options"}
        disabled={turnBusy || projectTransitioning}
        onClick={(event) => {
          optionsTriggerRef.current = event.currentTarget;
          toggle("options");
        }}
      >
        <Plus size={16} aria-hidden="true" />
      </button>
      {imageControl}
      {availableBinding ? (
        <button
          type="button"
          className={`composer__control-trigger composer__access-label${effectiveAccessMode === "workspaceWrite" ? " composer__access-label--write" : ""}`}
          aria-label="Access options"
          aria-description={sessionAccessModeCopy(effectiveAccessMode).label}
          title={sessionAccessModeCopy(effectiveAccessMode).label}
          aria-haspopup="dialog"
          aria-controls={optionsId}
          aria-expanded={active === "access"}
          disabled={turnBusy || projectTransitioning}
          onClick={(event) => {
            optionsTriggerRef.current = event.currentTarget;
            toggle("access");
          }}
        >
          <ShieldCheck size={13} aria-hidden="true" />
          <span className="composer__access-text">
            {effectiveAccessMode === "workspaceWrite"
              ? "Full access"
              : sessionAccessModeCopy(effectiveAccessMode).label}
          </span>
        </button>
      ) : null}
      <label className="composer__control-trigger composer__harness">
        <SearchablePicker
          ariaLabel="Harness"
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
        onToggle={() => toggle("model")}
        onDismiss={dismiss}
        choices={modelChoices}
        selectedId={currentModelId}
        modelLabel={modelLabel}
        effortLabel={modelLabel === "Model" ? null : effortLabel}
        contextWindow={contextWindow}
        effortOption={effortOption}
        disabled={turnBusy || projectTransitioning || !canPrompt}
        pending={modePending}
        onSelectModel={chooseModel}
        onSetOption={onSetSessionConfigOption}
      />
      {active === "options" || active === "access" ? (
        <MenuPopover
          id={optionsId}
          role="dialog"
          className="composer__popover composer__options"
          ariaLabel="Composer options menu"
          focusKey={active}
          open
          triggerRef={optionsTriggerRef}
          onDismiss={dismiss}
          gap={8}
        >
          <ChatSessionConfigControls
            sessionConfig={selectedSessionConfig}
            group="other"
            pending={modePending}
            disabled={turnBusy || projectTransitioning}
            error={modeError}
            onSetSessionConfigOption={onSetSessionConfigOption}
          />
          {projectName ? (
            <ProjectScopeChip
              projectName={projectName}
              disabled={projectTransitioning || turnBusy}
            />
          ) : null}
          {hasModes ? (
            <label
              ref={modeControlRef}
              className="select-control select-control--chip"
            >
              <Settings
                className="select-control__icon"
                size={14}
                aria-hidden="true"
              />
              <SearchablePicker
                ariaLabel="Session mode"
                portalTarget="closest-dialog"
                candidates={modeCandidates}
                disabled={turnBusy || projectTransitioning || modePending}
                onSelect={(modeId) => void onSetSessionMode(modeId)}
                selectedId={selectedSessionConfig?.modes?.currentModeId ?? null}
              />
              {modePending ? (
                <span className="select-control__reason" role="status">
                  Changing mode…
                </span>
              ) : null}
              {modeError ? (
                <span className="select-control__reason" role="alert">
                  {modeError}
                </span>
              ) : null}
            </label>
          ) : null}
          {availableBinding ? (
            <label className="select-control select-control--chip access-mode">
              <ShieldCheck
                className="select-control__icon"
                size={14}
                aria-hidden="true"
              />
              <SearchablePicker
                ariaLabel="Permission mode"
                portalTarget="closest-dialog"
                candidates={accessModeCandidates}
                disabled={!accessModeReady || turnBusy || projectTransitioning}
                footer={SESSION_ACCESS_MODE_SCOPE}
                onSelect={(mode) =>
                  void setAccessMode(mode as SessionAccessMode)
                }
                selectedId={accessMode}
              />
              {accessModeError ? (
                <span className="select-control__reason" role="alert">
                  {accessModeError}
                </span>
              ) : null}
            </label>
          ) : null}
        </MenuPopover>
      ) : null}
    </div>
  );
}

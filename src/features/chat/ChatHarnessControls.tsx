import { ShieldCheck } from "lucide-react";
import {
  useCallback,
  useLayoutEffect,
  useMemo,
  useRef,
  type RefObject,
} from "react";

import { useWorkbenchStore } from "../../app/store";
import type { AcpSessionConfig } from "../../domain/acpSessionConfig";
import { SearchablePicker } from "../../ui/SearchablePicker";

interface Props {
  availableBinding: boolean;
  canPrompt: boolean;
  sessionConfig: AcpSessionConfig | null;
  modePending: boolean;
  modeError: string | null;
  focused: boolean;
  onSetSessionMode: (modeId: string) => Promise<unknown>;
  onModeRemoved: () => void;
  modeControlRef?: RefObject<HTMLLabelElement | null>;
}

export function ChatHarnessControls({
  availableBinding,
  canPrompt,
  sessionConfig,
  modePending,
  modeError,
  focused,
  onSetSessionMode,
  onModeRemoved,
  modeControlRef: externalModeControlRef,
}: Props) {
  const harnesses = useWorkbenchStore((state) => state.harnesses);
  const selectedHarnessId = useWorkbenchStore(
    (state) => state.selectedHarnessId,
  );
  const models = useWorkbenchStore((state) => state.models);
  const selectedModelId = useWorkbenchStore((state) => state.selectedModelId);
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const selectHarness = useWorkbenchStore((state) => state.selectHarness);
  const selectModel = useWorkbenchStore((state) => state.selectModel);
  const harnessCandidates = useMemo(
    () =>
      harnesses
        .filter(
          (harness) =>
            harness.installState !== "notInstalled" ||
            harness.source === "development" ||
            harness.id === selectedHarnessId,
        )
        .map((harness) => ({
          disabledReason:
            harness.installState === "available"
              ? undefined
              : harness.installState === "notInstalled"
                ? "Harness is not installed."
                : "Harness is unavailable.",
          id: harness.id,
          label: `${harness.name}${harness.version ? ` — ${harness.version}` : ""}${
            harness.source === "development"
              ? " — test only"
              : harness.installState === "notInstalled"
                ? " — not installed"
                : harness.installState === "available"
                  ? ""
                  : " — unavailable"
          }`,
        })),
    [harnesses, selectedHarnessId],
  );
  const modelCandidates = useMemo(
    () => models.map((model) => ({ id: model.id, label: model.name })),
    [models],
  );
  const chooseHarness = useCallback(
    (id: string) => void selectHarness(id),
    [selectHarness],
  );
  const chooseModel = useCallback(
    (id: string) => selectModel(id),
    [selectModel],
  );
  const modelUnavailable = !canPrompt || models.length === 0;
  const internalModeControlRef = useRef<HTMLLabelElement>(null);
  const modeControlRef = externalModeControlRef ?? internalModeControlRef;
  const modeOptions = sessionConfig?.modes?.availableModes ?? [];
  const hasModes = !sessionConfig?.removed && modeOptions.length > 0;
  const modeCandidates = useMemo(
    () =>
      modeOptions.map((mode) => ({
        id: mode.id,
        label: mode.name || mode.id,
      })),
    [modeOptions],
  );
  const chooseMode = useCallback(
    (modeId: string) => void onSetSessionMode(modeId),
    [onSetSessionMode],
  );
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
  }, [focused, hasModes, onModeRemoved]);

  return (
    <div className="composer__controls">
      <label className="select-control select-control--chip">
        <span className="select-control__label">Harness</span>
        <SearchablePicker
          ariaLabel="Harness"
          candidates={harnessCandidates}
          disabled={projectTransitioning}
          onSelect={chooseHarness}
          selectedId={selectedHarnessId}
        />
      </label>
      <label className="select-control select-control--chip">
        <span className="select-control__label">Model</span>
        <SearchablePicker
          ariaLabel="Model"
          candidates={modelCandidates}
          disabled={projectTransitioning || modelUnavailable}
          onSelect={chooseModel}
          selectedId={selectedModelId}
        />
        {modelUnavailable ? (
          <span className="select-control__reason">
            Select a ready harness to load models.
          </span>
        ) : null}
      </label>
      {hasModes ? (
        <label
          ref={modeControlRef}
          className="select-control select-control--chip"
        >
          <span className="select-control__label">Mode</span>
          <SearchablePicker
            ariaLabel="Session mode"
            candidates={modeCandidates}
            onSelect={chooseMode}
            selectedId={sessionConfig?.modes?.currentModeId ?? null}
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
        <span className="access-mode" title="Permission requests fail closed">
          <ShieldCheck size={14} aria-hidden="true" />
          Read-only policy
        </span>
      ) : null}
    </div>
  );
}

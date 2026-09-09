import { ShieldCheck } from "lucide-react";
import { useCallback, useMemo } from "react";

import { useWorkbenchStore } from "../../app/store";
import { SearchablePicker } from "../../ui/SearchablePicker";

interface Props {
  availableBinding: boolean;
  canPrompt: boolean;
}

export function ChatHarnessControls({ availableBinding, canPrompt }: Props) {
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
      {availableBinding ? (
        <span className="access-mode" title="Permission requests fail closed">
          <ShieldCheck size={14} aria-hidden="true" />
          Read-only policy
        </span>
      ) : null}
    </div>
  );
}

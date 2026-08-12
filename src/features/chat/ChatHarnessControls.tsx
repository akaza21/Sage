import { ChevronDown, ShieldCheck } from "lucide-react";

import { useWorkbenchStore } from "../../app/store";

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
  const visibleHarnesses = harnesses.filter(
    (harness) =>
      harness.installState === "available" ||
      harness.source === "development" ||
      harness.id === selectedHarnessId,
  );

  return (
    <div className="composer__controls">
      <label className="select-control">
        <span className="sr-only">Harness</span>
        <select
          value={selectedHarnessId ?? ""}
          disabled={projectTransitioning}
          onChange={(event) => void selectHarness(event.target.value)}
        >
          {visibleHarnesses.map((harness) => (
            <option
              key={harness.id}
              value={harness.id}
              disabled={harness.installState !== "available"}
            >
              {harness.name}
              {harness.source === "development"
                ? " — test only"
                : harness.installState !== "available"
                  ? " — unavailable"
                  : ""}
            </option>
          ))}
        </select>
        <ChevronDown size={14} aria-hidden="true" />
      </label>
      <label className="select-control">
        <span className="sr-only">Model</span>
        <select
          value={selectedModelId ?? ""}
          disabled={projectTransitioning || !canPrompt || models.length === 0}
          onChange={(event) => selectModel(event.target.value)}
        >
          {models.length === 0 ? (
            <option value="">Harness default</option>
          ) : (
            models.map((model) => (
              <option key={model.id} value={model.id}>
                {model.name}
              </option>
            ))
          )}
        </select>
        <ChevronDown size={14} aria-hidden="true" />
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

import { useId, useMemo, useRef, useState } from "react";
import { useSessionAccessModeStore } from "../../app/sessionAccessModeStore";
import {
  SESSION_ACCESS_MODE_SCOPE,
  type SessionAccessMode,
} from "../../domain/sessionAccessMode";
import { ModalDialog } from "../../ui/ModalDialog";
import { useWorkbenchStore } from "../../app/store";
import { useSessionAccessSelection } from "../../app/useSessionAccessSelection";
import { harnessAvailability } from "../chat/harnessAvailability";
import { LazyHarnessNotice } from "../chat/lazyParts";
import { composerModelName } from "../chat/composerModelName";
import { SettingsRow } from "./SettingsRow";

const MODEL_RESULT_LIMIT = 200;

export function AgentChatSettings() {
  const harnesses = useWorkbenchStore((state) => state.harnesses);
  const harnessId = useWorkbenchStore((state) => state.selectedHarnessId);
  const models = useWorkbenchStore((state) => state.models);
  const modelId = useWorkbenchStore((state) => state.selectedModelId);
  const phase = useWorkbenchStore((state) => state.harnessDiscoveryPhase);
  const project = useWorkbenchStore((state) => state.project);
  const transitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const selectHarness = useWorkbenchStore((state) => state.selectHarness);
  const selectModel = useWorkbenchStore((state) => state.selectModel);
  const harness = harnesses.find((candidate) => candidate.id === harnessId);
  const access = useSessionAccessSelection(
    harness?.bindingId,
    harness?.accessModes,
  );
  const disabled =
    !project || transitioning || phase === "checking" || phase === "waiting";
  const confirmationId = useId();
  const accessRef = useRef<HTMLSelectElement>(null);
  const [confirmation, setConfirmation] = useState<{
    harnessId: string;
    generation: number;
  } | null>(null);
  const [saving, setSaving] = useState(false);
  const [failedMode, setFailedMode] = useState<SessionAccessMode | null>(null);
  const generation = useWorkbenchStore((state) => state.projectGeneration);
  if (
    confirmation &&
    (confirmation.harnessId !== harnessId ||
      confirmation.generation !== generation ||
      transitioning)
  )
    setConfirmation(null);
  const writeAccess = async (mode: SessionAccessMode) => {
    if (!harness?.bindingId || disabled) return;
    setSaving(true);
    access.setMode(harness.bindingId, mode);
    const saved = await useSessionAccessModeStore.getState().settle();
    setSaving(false);
    setFailedMode(saved ? null : mode);
  };
  const dismissConfirmation = () => {
    setConfirmation(null);
    accessRef.current?.focus();
  };
  const [filter, setFilter] = useState({ harnessId, query: "" });
  const query = filter.harnessId === harnessId ? filter.query : "";
  const choices = useMemo(() => {
    const labels = models.map((model) => composerModelName(model.name));
    const counts = new Map<string, number>();
    for (const label of labels) counts.set(label, (counts.get(label) ?? 0) + 1);
    return models.map((model, index) => ({
      ...model,
      label:
        (counts.get(labels[index]) ?? 0) > 1
          ? `${labels[index]} (${model.id})`
          : labels[index],
    }));
  }, [models]);
  const visible = choices
    .filter((model) =>
      `${model.label} ${model.id}`
        .toLowerCase()
        .includes(query.trim().toLowerCase()),
    )
    .slice(0, MODEL_RESULT_LIMIT);
  const selected = choices.find((model) => model.id === modelId);
  if (selected && !visible.some((model) => model.id === modelId))
    visible.splice(MODEL_RESULT_LIMIT - 1, 1, selected);
  return (
    <>
      <p>
        Harness and model use this window's current workbench selection, not a
        saved app-wide default. Existing chat sessions keep their own
        configuration.
      </p>
      <div className="agent-settings__card">
        <SettingsRow
          title="Current harness"
          description="Choose the harness before choosing one of its discovered models."
        >
          <select
            aria-label="Current harness"
            className="input"
            disabled={disabled}
            value={harnessId ?? ""}
            onChange={(event) => void selectHarness(event.target.value)}
          >
            <option value="" disabled>
              Select a harness
            </option>
            {harnesses.slice(0, 32).map((item) => (
              <option
                key={item.id}
                value={item.id}
                disabled={Boolean(harnessAvailability(item).disabledReason)}
              >
                {item.name}
              </option>
            ))}
          </select>
        </SettingsRow>
        <SettingsRow
          title="Current model"
          description={
            models.length
              ? "Only models reported by the selected harness are available."
              : "Select an installed harness and complete discovery or sign-in to load models."
          }
        >
          <input
            type="search"
            className="input input--compact"
            aria-label="Search models"
            maxLength={256}
            disabled={disabled || phase !== "ready" || !models.length}
            placeholder="Search models"
            value={query}
            onChange={(event) =>
              setFilter({ harnessId, query: event.target.value })
            }
          />
          <select
            aria-label="Current model"
            className="input"
            disabled={disabled || phase !== "ready" || !models.length}
            value={modelId ?? ""}
            onChange={(event) => selectModel(event.target.value)}
          >
            <option value="" disabled>
              No model selected
            </option>
            {visible.map((model) => (
              <option key={model.id} value={model.id}>
                {model.label}
              </option>
            ))}
          </select>
          {!visible.length ? <p role="status">No matching models.</p> : null}
        </SettingsRow>
        <SettingsRow
          title="Default access"
          description={
            access.reason ??
            "Saved per harness. Applies from your next message with this harness."
          }
        >
          <select
            aria-label="Default access"
            ref={accessRef}
            className="input"
            disabled={
              disabled ||
              saving ||
              !access.ready ||
              !harness?.bindingId ||
              !access.candidates.length
            }
            value={access.effective ?? ""}
            onChange={(event) => {
              const mode = access.candidates.find(
                (candidate) => candidate.id === event.target.value,
              );
              if (!harness?.bindingId || !mode) return;
              if (mode.id === "workspaceWrite" && access.effective !== mode.id)
                setConfirmation({ harnessId: harness.id, generation });
              else void writeAccess(mode.id);
            }}
          >
            <option value="" disabled>
              No supported mode selected
            </option>
            {access.candidates.map((mode) => (
              <option key={mode.id} value={mode.id}>
                {mode.label}
              </option>
            ))}
          </select>
          {saving ? <p role="status">Saving access…</p> : null}
          {access.error ? <p role="alert">{access.error}</p> : null}
          {failedMode && access.error ? (
            <button
              className="button button--compact"
              type="button"
              disabled={saving}
              onClick={() => {
                if (failedMode === "workspaceWrite" && harness)
                  setConfirmation({ harnessId: harness.id, generation });
                else void writeAccess(failedMode);
              }}
            >
              Retry access save
            </button>
          ) : null}
        </SettingsRow>
      </div>
      {confirmation &&
      confirmation.harnessId === harnessId &&
      confirmation.generation === generation &&
      !transitioning ? (
        <ModalDialog
          labelledBy={confirmationId}
          onDismiss={dismissConfirmation}
          className="dialog--scrim"
        >
          <h2 id={confirmationId}>
            Switch to{" "}
            {
              access.candidates.find((mode) => mode.id === "workspaceWrite")
                ?.label
            }
            ?
          </h2>
          <p>
            {
              access.candidates.find((mode) => mode.id === "workspaceWrite")
                ?.description
            }
          </p>
          <p>{SESSION_ACCESS_MODE_SCOPE}</p>
          <div className="dirty-dialog__actions">
            <button
              autoFocus
              type="button"
              className="button button--compact"
              onClick={dismissConfirmation}
            >
              Cancel
            </button>
            <button
              type="button"
              className="button button--compact"
              onClick={() => {
                const current = useWorkbenchStore.getState();
                if (
                  current.selectedHarnessId === confirmation.harnessId &&
                  current.projectGeneration === confirmation.generation &&
                  !current.projectTransitioning &&
                  access.candidates.some((mode) => mode.id === "workspaceWrite")
                )
                  void writeAccess("workspaceWrite");
                dismissConfirmation();
              }}
            >
              Switch access
            </button>
          </div>
        </ModalDialog>
      ) : null}
      <LazyHarnessNotice />
      {!project ? (
        <p role="status">Open a project to select a harness and model.</p>
      ) : null}
    </>
  );
}

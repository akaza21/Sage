import { useState } from "react";
import { useWorkbenchStore } from "../../app/store";
import { errorMessage } from "../../domain/errors";
import { PickerLogo } from "../../ui/PickerLogo";
import { RefreshCw } from "../../ui/leanIcons";
import { refreshHarnessDiscovery } from "../harness/refreshHarnessDiscovery";
import { useAgentSettingsState } from "../settings/agentSettingsState";
import "./composer-harness-extras.css";

/** Harnesses the menu names as not installed; the rest are its options. */
const MAX_MISSING = 16;

/**
 * The harness menu's tail: harnesses discovery knows but this computer lacks,
 * each with Install (Settings' harness page, where installing is explained),
 * then the same discovery refresh Settings runs.
 */
export function ComposerHarnessMenuExtras({
  disabled = false,
}: {
  disabled?: boolean;
}) {
  const harnesses = useWorkbenchStore((state) => state.harnesses);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const missing = harnesses
    .filter((harness) => harness.installState === "notInstalled")
    .slice(0, MAX_MISSING);
  const refresh = async () => {
    setRefreshing(true);
    setError(null);
    try {
      await refreshHarnessDiscovery();
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setRefreshing(false);
    }
  };
  return (
    <div className="composer__harness-extras">
      {missing.length > 0 ? (
        <div role="group" aria-label="Not installed">
          <p className="composer__harness-group-label" aria-hidden="true">
            Not installed
          </p>
          {missing.map((harness) => (
            <div className="composer__harness-missing" key={harness.id}>
              <PickerLogo icon={harness.brand ?? "generic"} />
              <span>{harness.name}</span>
              <button
                type="button"
                className="composer__harness-install"
                aria-label={`Install ${harness.name}`}
                onClick={() =>
                  useAgentSettingsState.getState().open("Harnesses")
                }
              >
                Install
              </button>
            </div>
          ))}
        </div>
      ) : null}
      <button
        type="button"
        className="composer__harness-refresh"
        disabled={disabled || refreshing}
        onClick={() => void refresh()}
      >
        <RefreshCw size={14} aria-hidden="true" />
        {refreshing ? "Checking harnesses…" : "Refresh harnesses"}
      </button>
      {error ? (
        <p className="composer__harness-error" role="alert">
          {error}
        </p>
      ) : null}
    </div>
  );
}

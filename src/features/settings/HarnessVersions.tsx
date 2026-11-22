import { useEffect, useState } from "react";

import { errorMessage } from "../../domain/errors";
import type { HarnessDefinition } from "../../domain/harness";
import {
  harnessSetUpdateCheck,
  harnessUpdateCheck,
} from "../../ports/nativeHarnessUpdateCheckPort";
import { SettingsRow } from "./SettingsRow";

/** Loaded with Settings › Harnesses only; see `AgentHarnessSettings`. */
export default function HarnessUpdateCheckSetting() {
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let current = true;
    harnessUpdateCheck()
      .then((value) => current && setEnabled(value))
      .catch((cause) => current && setError(errorMessage(cause)));
    return () => {
      current = false;
    };
  }, []);
  const change = (next: boolean) => {
    setSaving(true);
    setError(null);
    harnessSetUpdateCheck(next)
      .then(setEnabled)
      .catch((cause) => setError(errorMessage(cause)))
      .finally(() => setSaving(false));
  };
  return (
    <div className="agent-settings__card">
      <SettingsRow
        title="Check npm for new harness versions"
        description="Every 6 hours Sage asks npm, through your own Node.js, for the latest release of each installed harness. Off: Sage runs no npm command."
      >
        <input
          className="switch"
          type="checkbox"
          role="switch"
          aria-label="Check npm for new harness versions"
          checked={enabled ?? false}
          disabled={enabled === null || saving}
          onChange={(event) => change(event.target.checked)}
        />
      </SettingsRow>
      {error ? <p role="alert">{error}</p> : null}
    </div>
  );
}

/** Installed, Sage runs and Latest, why they differ, and what an update adds. */
export function HarnessVersionDetails({
  harness,
}: {
  harness: HarnessDefinition;
}) {
  const { name, version, update, runsVersion, directFromVersion } = harness;
  if (!version) return null;
  const available = harness.installState === "available";
  return (
    <>
      <p>
        Installed {version} · Sage runs{" "}
        {available ? (runsVersion ?? version) : "nothing yet"} · Latest{" "}
        {harness.latestVersion ?? "not checked"}
      </p>
      {available && directFromVersion ? (
        <p>
          Sage runs {runsVersion ? `the ${runsVersion} release` : "the release"}{" "}
          bundled with its adapter, because {version} is older than{" "}
          {directFromVersion}. Update {name} to {directFromVersion} or newer and
          Sage runs it directly.
        </p>
      ) : null}
      {update ? (
        <p>
          Update available: {version} → {update.latestVersion}. Run{" "}
          <code>{update.command}</code> in a terminal, then choose Refresh
          discovery.
        </p>
      ) : null}
      {update?.unlocks?.length ? (
        <>
          <p>Updating adds:</p>
          <ul aria-label={`${name} ${update.latestVersion} adds`}>
            {update.unlocks.slice(0, 16).map((command) => (
              <li key={command}>
                <code>/{command}</code>
              </li>
            ))}
          </ul>
        </>
      ) : null}
    </>
  );
}

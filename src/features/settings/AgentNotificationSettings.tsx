import { useEffect, useState } from "react";
import { errorMessage } from "../../domain/errors";
import { isTauriRuntime } from "../../ports/nativeInvoke";
import {
  nativeNotificationPort,
  type NotificationPreference,
} from "../../ports/nativeNotificationPort";
import { SettingsRow } from "./SettingsRow";

const PREFERENCES: readonly {
  value: NotificationPreference;
  label: string;
}[] = [
  { value: "backgroundTurns", label: "When the chat isn't in view" },
  { value: "off", label: "Off" },
];

export default function AgentNotificationSettings() {
  const native = isTauriRuntime();
  const [preference, setPreference] = useState<NotificationPreference | null>(
    null,
  );
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!native) return;
    let current = true;
    void nativeNotificationPort
      .getPreference()
      .then((value) => {
        if (current) setPreference(value);
      })
      .catch((cause) => {
        if (current) setError(errorMessage(cause));
      });
    return () => {
      current = false;
    };
  }, [native]);
  if (!native)
    return <p role="status">Notifications are available in the desktop app.</p>;
  const save = (value: NotificationPreference) => {
    setSaving(true);
    setError(null);
    void nativeNotificationPort
      .setPreference(value)
      .then(setPreference)
      .catch((cause) => setError(errorMessage(cause)))
      .finally(() => setSaving(false));
  };
  return (
    <>
      <div className="agent-settings__card">
        <SettingsRow
          title="Turn notifications"
          description="Show a system notification when a turn completes, fails or needs approval in a chat you aren't viewing."
        >
          <select
            aria-label="Turn notifications"
            className="select"
            disabled={preference === null || saving}
            value={preference ?? ""}
            onChange={(event) => {
              const next = PREFERENCES.find(
                (option) => option.value === event.target.value,
              );
              if (next) save(next.value);
            }}
          >
            {preference === null ? (
              <option value="" disabled>
                Loading...
              </option>
            ) : null}
            {PREFERENCES.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </SettingsRow>
      </div>
      {error ? <p role="alert">{error}</p> : null}
    </>
  );
}

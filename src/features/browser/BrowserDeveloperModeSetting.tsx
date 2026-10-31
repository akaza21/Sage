import { useEffect, useState } from "react";

import {
  browserDeveloperMode,
  browserSetDeveloperMode,
  type BrowserDeveloperMode,
} from "../../ports/nativeBrowserPreferencesPort";

const TOGGLE = "browser-developer-mode";

/**
 * Settings › Browser › Developer mode, after Codex ref 65 (ADR-0009b). Rust
 * owns the switch; this reads it and writes it, with no copy of its own.
 * Turning it on grants nothing: each browser session still needs the user's
 * approval on its own card. Hidden where the browser has no CDP.
 */
export function BrowserDeveloperModeSetting() {
  const [mode, setMode] = useState<BrowserDeveloperMode | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let current = true;
    browserDeveloperMode()
      .then((next) => {
        if (current) setMode(next);
      })
      .catch(() => {
        if (current) setError("Couldn't read the developer mode setting.");
      });
    return () => {
      current = false;
    };
  }, []);
  if (!mode?.available && !error) return null;
  const change = (enabled: boolean) => {
    setBusy(true);
    setError(null);
    browserSetDeveloperMode(enabled)
      .then(setMode)
      .catch(() => setError("Couldn't change developer mode. Try again."))
      .finally(() => setBusy(false));
  };
  return (
    <section
      className="settings-surface__group"
      role="group"
      aria-labelledby="browser-developer-heading"
    >
      <h3 id="browser-developer-heading">Developer mode</h3>
      {mode ? (
        <div className="settings-surface__row">
          <div className="settings-surface__copy">
            <strong className="settings-surface__risk">Elevated risk</strong>
            <label htmlFor={TOGGLE}>Enable full CDP access</label>
            <p id={`${TOGGLE}-description`}>
              Let agents use the full Chrome DevTools Protocol on a browser tab
              after you approve it for that tab and site. Full CDP access can
              run scripts on the page and read the site&apos;s cookies and
              session data. Stop or Take over ends it.
            </p>
            <small id={`${TOGGLE}-default`}>Default: Off</small>
            {error ? (
              <span className="settings-surface__error" role="alert">
                {error}
              </span>
            ) : null}
          </div>
          <div className="settings-surface__toggle">
            <span>{mode.enabled ? "On" : "Off"}</span>
            <input
              id={TOGGLE}
              type="checkbox"
              checked={mode.enabled}
              disabled={busy}
              aria-describedby={`${TOGGLE}-description ${TOGGLE}-default`}
              onChange={(event) => change(event.target.checked)}
            />
          </div>
        </div>
      ) : (
        <p className="settings-surface__error" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}

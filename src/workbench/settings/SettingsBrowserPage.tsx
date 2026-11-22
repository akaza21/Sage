import { useEffect } from "react";

import { BrowserClearData } from "../../features/browser/BrowserClearData";
import { BrowserDeveloperModeSetting } from "../../features/browser/BrowserDeveloperModeSetting";
import { BrowserDownloadFolder } from "../../features/browser/BrowserDownloadFolder";
import { BrowserSitePermissionsGroup } from "../../features/browser/BrowserSiteSettings";
import {
  DEFAULT_BROWSER_PREFERENCES,
  ensureBrowserPreferences,
  useBrowserPreferencesStore,
} from "../../features/browser/browserPreferencesStore";
import { isTauriRuntime } from "../../ports/nativeInvoke";

const SHOW_FULL_URL = "browser-show-full-url";

export function SettingsBrowserPage({ visible }: { visible: Set<string> }) {
  const desktop = isTauriRuntime();
  const preferences = useBrowserPreferencesStore((state) => state.preferences);
  const ready = useBrowserPreferencesStore((state) => state.ready);
  const error = useBrowserPreferencesStore((state) => state.error);
  const restore = useBrowserPreferencesStore((state) => state.restore);
  const setShowFullUrl = useBrowserPreferencesStore(
    (state) => state.setShowFullUrl,
  );
  useEffect(() => {
    if (desktop) ensureBrowserPreferences();
  }, [desktop]);

  return (
    <section
      className="settings-surface__section"
      aria-labelledby="browser-settings-heading"
    >
      <header className="settings-surface__section-header">
        <h2 id="browser-settings-heading">Browser</h2>
        <p>These choices apply to every tab in the built-in browser.</p>
      </header>
      {!desktop ? (
        <p className="settings-surface__group-note" role="status">
          Browser settings are available in the Sage desktop app.
        </p>
      ) : (
        <>
          <section
            className="settings-surface__group"
            role="group"
            aria-labelledby="browser-general-heading"
          >
            <h3 id="browser-general-heading">General</h3>
            {error ? (
              <div className="settings-surface__error" role="alert">
                <span>{error}</span>
                {!ready ? (
                  <button
                    className="button button--compact"
                    type="button"
                    onClick={() => void restore()}
                  >
                    Retry
                  </button>
                ) : null}
              </div>
            ) : null}
            <div className="settings-surface__rows">
              {visible.has("browser.showFullUrl") ? (
                <div className="settings-surface__row">
                  <div className="settings-surface__copy">
                    <label htmlFor={SHOW_FULL_URL}>Show full URL</label>
                    <p id={`${SHOW_FULL_URL}-description`}>
                      Include the path, query, and fragment in the address bar.
                    </p>
                    <small id={`${SHOW_FULL_URL}-default`}>
                      Default:{" "}
                      {DEFAULT_BROWSER_PREFERENCES.showFullUrl ? "On" : "Off"}
                    </small>
                  </div>
                  <div className="settings-surface__toggle">
                    <input
                      id={SHOW_FULL_URL}
                      className="switch"
                      type="checkbox"
                      role="switch"
                      checked={preferences.showFullUrl}
                      disabled={!ready}
                      aria-describedby={`${SHOW_FULL_URL}-description ${SHOW_FULL_URL}-default`}
                      onChange={(event) => setShowFullUrl(event.target.checked)}
                    />
                  </div>
                </div>
              ) : null}
              {visible.has("browser.clearData") ? (
                <div className="settings-surface__row">
                  <div className="settings-surface__copy">
                    <span>Browsing data</span>
                    <p id="browser-clear-data-description">
                      Clear browsing history, cookies, site data, and cache from
                      the built-in browser. Every browser tab closes.
                    </p>
                  </div>
                  <BrowserClearData
                    buttonClassName="button button--compact"
                    describedBy="browser-clear-data-description"
                  />
                </div>
              ) : null}
            </div>
          </section>
          {visible.has("browser.developerMode") ? (
            <BrowserDeveloperModeSetting />
          ) : null}
        </>
      )}
      {desktop && visible.has("browser.downloadFolder") ? (
        <section
          className="settings-surface__group"
          role="group"
          aria-labelledby="browser-downloads-heading"
        >
          <h3 id="browser-downloads-heading">Downloads</h3>
          <BrowserDownloadFolder />
        </section>
      ) : null}
      {desktop && visible.has("browser.sitePermissions") ? (
        <BrowserSitePermissionsGroup />
      ) : null}
    </section>
  );
}

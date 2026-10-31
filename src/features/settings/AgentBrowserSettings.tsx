import { useEffect, useState } from "react";
import { useWorkbenchStore } from "../../app/store";
import { browserHistory, browserLiveTabs } from "../../ports/nativeBrowserPort";
import { isTauriRuntime } from "../../ports/nativeInvoke";
import { errorMessage } from "../../domain/errors";
import { BrowserClearData } from "../browser/BrowserClearData";
import { useBrowserDownloads } from "../browser/useBrowserDownloads";
import { LazyModuleBoundary } from "../../ui/LazyModuleBoundary";
import { BrowserSitePermissions } from "../browser/BrowserSitePermissions";
import { useAgentSettingsState } from "./agentSettingsState";
import { SettingsRow } from "./SettingsRow";

const loadBrowserPreferences = () =>
  import("../../workbench/settings/SettingsBrowserPage").then(
    ({ SettingsBrowserPage }) => ({ default: SettingsBrowserPage }),
  );
const generalPreferences = new Set(["browser.showFullUrl"]);

const loadDownloadsPanel = () => import("../browser/BrowserDownloadsPanel");

function LiveTabDownloads({ tabId }: { tabId: string }) {
  const downloads = useBrowserDownloads(tabId);
  return (
    <LazyModuleBoundary
      label="Downloads"
      load={loadDownloadsPanel}
      render={(DownloadsPanel) => (
        <DownloadsPanel tabId={tabId} downloads={downloads} />
      )}
    />
  );
}

function BrowsingHistory({ tabId }: { tabId: string }) {
  const [entries, setEntries] = useState<string[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let current = true;
    void browserHistory(tabId)
      .then((history) => {
        if (current) setEntries(history.slice(0, 100));
      })
      .catch((cause) => {
        if (current) setError(errorMessage(cause));
      });
    return () => {
      current = false;
    };
  }, [tabId]);
  return (
    <section aria-label="Browsing history">
      <p>
        Recent history for this live tab only; Sage does not expose a persistent
        global history.
      </p>
      {error ? (
        <p role="alert">{error}</p>
      ) : entries === null ? (
        <p role="status">Loading browsing history...</p>
      ) : entries.length ? (
        <ul>
          {entries.map((url, index) => (
            <li key={`${index}:${url}`}>{url}</li>
          ))}
        </ul>
      ) : (
        <p>No history for this tab yet.</p>
      )}
    </section>
  );
}

function LiveTabSettings({
  tabId,
  showSitePermissions,
}: {
  tabId: string;
  showSitePermissions: boolean;
}) {
  const [permissions, setPermissions] = useState(showSitePermissions);
  const [history, setHistory] = useState(false);
  const [downloads, setDownloads] = useState(false);
  return (
    <div className="agent-settings__card">
      <SettingsRow
        title="Site permissions"
        description="Review and revoke permissions for this live tab. Revoking reloads the page."
      >
        <button
          type="button"
          className="button button--compact"
          aria-expanded={permissions}
          onClick={() => setPermissions(!permissions)}
        >
          Manage site permissions
        </button>
      </SettingsRow>
      {permissions ? <BrowserSitePermissions tabId={tabId} showEmpty /> : null}
      <SettingsRow
        title="Browsing history"
        description="Recent pages visited by this browser tab."
      >
        <button
          type="button"
          className="button button--compact"
          aria-expanded={history}
          onClick={() => setHistory(!history)}
        >
          Manage browsing history
        </button>
      </SettingsRow>
      {history ? <BrowsingHistory tabId={tabId} /> : null}
      <SettingsRow
        title="Downloads"
        description="Review downloads, cancel active downloads and show completed files in their folder."
      >
        <button
          type="button"
          className="button button--compact"
          aria-expanded={downloads}
          onClick={() => setDownloads(!downloads)}
        >
          Manage downloads
        </button>
      </SettingsRow>
      {downloads ? <LiveTabDownloads tabId={tabId} /> : null}
    </div>
  );
}

export function AgentBrowserSettings() {
  const tabs = useWorkbenchStore((state) => state.tabs);
  const clearNotice = useWorkbenchStore((state) => state.browserClearNotice);
  const showSitePermissions = useAgentSettingsState(
    (state) => state.showSitePermissions,
  );
  const requestedId = useAgentSettingsState((state) => state.browserTabId);
  const [chosenId, setChosenId] = useState<string | null>(null);
  const [liveIds, setLiveIds] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);
  const [revision, setRevision] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const native = isTauriRuntime();
  useEffect(() => {
    if (!native) return;
    let current = true;
    void browserLiveTabs()
      .then((ids) => {
        if (current) {
          setLiveIds(ids.slice(0, 32));
          setLoading(false);
          setError(null);
        }
      })
      .catch((cause) => {
        if (current) {
          setError(errorMessage(cause));
          setLoading(false);
        }
      });
    return () => {
      current = false;
    };
  }, [native, revision, clearNotice]);
  if (!native)
    return (
      <p role="status">
        Browser settings are available in the desktop app. No browser data is
        simulated here.
      </p>
    );
  const tabId =
    [chosenId, requestedId].find((id) => id && liveIds.includes(id)) ??
    liveIds[0];
  return (
    <>
      <LazyModuleBoundary
        label="Browser preferences"
        load={loadBrowserPreferences}
        render={(SettingsBrowserPage) => (
          <SettingsBrowserPage visible={generalPreferences} />
        )}
      />
      <p>
        Manage the built-in browser's real data and permissions. History,
        downloads and permission status are scoped to a live tab. Closed-tab
        history and download management are not available yet.
      </p>
      <div className="agent-settings__card">
        <SettingsRow
          title="Browsing data"
          description="Clear browser history, cookies and site storage. All browser tabs close and agent browser access is revoked; Sage and Preview data stay intact."
        >
          <BrowserClearData />
        </SettingsRow>
      </div>
      <label>
        Browser tab{" "}
        <select
          className="input"
          aria-label="Browser tab"
          disabled={loading || !liveIds.length}
          value={tabId ?? ""}
          onChange={(event) => setChosenId(event.target.value)}
        >
          {!liveIds.length ? (
            <option value="">No live browser tabs</option>
          ) : null}
          {liveIds.map((id) => (
            <option value={id} key={id}>
              {tabs.find((tab) => tab.id === id)?.title ?? id}
            </option>
          ))}
        </select>
      </label>
      <button
        type="button"
        className="button button--compact"
        disabled={loading}
        onClick={() => {
          setLoading(true);
          setRevision((value) => value + 1);
        }}
      >
        Refresh browser tabs
      </button>
      {error ? <p role="alert">{error}</p> : null}
      {loading ? (
        <p role="status">Loading browser tabs...</p>
      ) : tabId ? (
        <LiveTabSettings
          key={tabId}
          tabId={tabId}
          showSitePermissions={showSitePermissions}
        />
      ) : (
        <p role="status">
          Open a browser tab in Files and preview to manage its history, site
          permissions and downloads.
        </p>
      )}
    </>
  );
}

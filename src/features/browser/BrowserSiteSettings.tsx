import { useCallback, useEffect, useRef, useState } from "react";

import {
  browserSiteReloadTabs,
  browserSiteSettingReset,
  browserSiteSettingSet,
  browserSiteSettings,
  browserSiteSettingsResetAll,
  type BrowserSiteSetting,
} from "../../ports/nativeBrowserSiteSettingsPort";
import type { BrowserPermissionKind } from "../../ports/nativeBrowserPort";
import {
  admitBrowserSiteSettings,
  admitSiteOrigins,
  admitSiteSettingsUpdate,
  SITE_PERMISSION_LABELS,
} from "./browserSiteSettingsModel";
import { useSiteReloadFailures } from "./siteReloadFailures";

const LOAD_FAILURE = "Site permissions could not be loaded.";
const SAVE_FAILURE = "The site permission could not be changed.";
const RELOAD_FAILURE = "The tabs could not be reloaded.";

const rowKey = (row: Pick<BrowserSiteSetting, "origin" | "kind">) =>
  `${row.origin} ${row.kind}`;

/**
 * Settings > Browser > Site permissions: every answer the user saved for a
 * site, with a control to switch it between Allow and Block and a reset that
 * makes the site ask again. Rust owns the list; this view only mirrors it.
 */
export function BrowserSiteSettings({ describedBy }: { describedBy: string }) {
  const [rows, setRows] = useState<BrowserSiteSetting[]>([]);
  const [ready, setReady] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const unreloaded = useSiteReloadFailures((state) => state.sites);
  const addUnreloaded = useSiteReloadFailures((state) => state.add);
  const replaceUnreloaded = useSiteReloadFailures((state) => state.replace);
  // Counts change events, so a reply that a newer event overtook is dropped.
  const events = useRef(0);

  const retry = useCallback(async () => {
    const seen = events.current;
    try {
      const loaded = admitBrowserSiteSettings(await browserSiteSettings());
      if (seen === events.current) setRows(loaded);
      setReady(true);
      setError(null);
    } catch {
      setError(LOAD_FAILURE);
    }
  }, []);

  useEffect(() => {
    let mounted = true;
    let sawEvent = false;
    let unlisten: (() => void) | undefined;
    // Subscribe before the first read, so a change made in a browser tab
    // between the two is not lost; an event that lands before the read
    // returns is newer than it.
    void (async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        const dispose = await listen<unknown>(
          "browser:site-settings",
          (event) => {
            if (!mounted) return;
            sawEvent = true;
            events.current += 1;
            setRows(admitBrowserSiteSettings(event.payload));
            setReady(true);
            setError(null);
          },
        );
        if (mounted) unlisten = dispose;
        else dispose();
      } catch {
        // Without live updates the list still loads; Settings refreshes on
        // its own changes.
      }
      if (!mounted) return;
      try {
        const loaded = admitBrowserSiteSettings(await browserSiteSettings());
        if (!mounted || sawEvent) return;
        setRows(loaded);
        setReady(true);
      } catch {
        if (mounted && !sawEvent) setError(LOAD_FAILURE);
      }
    })();
    return () => {
      mounted = false;
      unlisten?.();
    };
  }, []);

  async function change(key: string, action: () => Promise<unknown>) {
    if (busy !== null) return;
    setBusy(key);
    setError(null);
    const seen = events.current;
    try {
      const update = admitSiteSettingsUpdate(await action());
      if (seen === events.current) setRows(update.settings);
      // Earlier failures stay until Reload tabs confirms them; a later
      // change elsewhere does not prove those tabs lost access.
      addUnreloaded(update.reloadFailed);
    } catch {
      setError(SAVE_FAILURE);
    } finally {
      setBusy(null);
    }
  }

  async function reloadAgain() {
    if (busy !== null) return;
    setBusy("reload");
    setError(null);
    try {
      replaceUnreloaded(
        admitSiteOrigins(await browserSiteReloadTabs([...unreloaded])),
      );
    } catch {
      setError(RELOAD_FAILURE);
    } finally {
      setBusy(null);
    }
  }

  function resetAll() {
    if (
      !window.confirm(
        "Reset every site permission? Sites will ask again, and pages that were allowed access reload.",
      )
    ) {
      return;
    }
    void change("all", browserSiteSettingsResetAll);
  }

  const label = (kind: BrowserPermissionKind) => SITE_PERMISSION_LABELS[kind];

  return (
    <div className="settings-surface__site-permissions">
      {error ? (
        <div className="settings-surface__error" role="alert">
          <span>{error}</span>
          {!ready ? (
            <button
              className="button button--compact"
              type="button"
              onClick={() => void retry()}
            >
              Retry
            </button>
          ) : null}
        </div>
      ) : null}
      {unreloaded.length ? (
        <div className="settings-surface__error" role="alert">
          <span>
            Saved, but tabs on {unreloaded.join(", ")} could not reload and may
            keep access until they do.
          </span>
          <button
            className="button button--compact"
            type="button"
            disabled={busy !== null}
            onClick={() => void reloadAgain()}
          >
            Reload tabs
          </button>
        </div>
      ) : null}
      {!ready ? null : rows.length === 0 ? (
        <p className="settings-surface__group-note" role="status">
          No site permissions saved yet.
        </p>
      ) : (
        <>
          <table aria-describedby={describedBy}>
            <thead>
              <tr>
                <th scope="col">Site</th>
                <th scope="col">Permission</th>
                <th scope="col">Setting</th>
                <th scope="col">
                  <span className="sr-only">Reset</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => {
                const key = rowKey(row);
                const name = `${label(row.kind)} for ${row.origin}`;
                return (
                  <tr key={key}>
                    <th scope="row" title={row.origin}>
                      {row.origin}
                    </th>
                    <td>{label(row.kind)}</td>
                    <td>
                      <select
                        className="select"
                        aria-label={name}
                        value={row.decision}
                        disabled={busy !== null}
                        onChange={(event) => {
                          const decision =
                            event.target.value === "allow" ? "allow" : "deny";
                          void change(key, () =>
                            browserSiteSettingSet(
                              row.origin,
                              row.kind,
                              decision,
                            ),
                          );
                        }}
                      >
                        <option value="allow">Allow</option>
                        <option value="deny">Block</option>
                      </select>
                    </td>
                    <td>
                      <button
                        className="button button--compact"
                        type="button"
                        aria-label={`Reset ${name}`}
                        disabled={busy !== null}
                        onClick={() =>
                          void change(key, () =>
                            browserSiteSettingReset(row.origin, row.kind),
                          )
                        }
                      >
                        Reset
                      </button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
          <div className="settings-surface__site-permissions-footer">
            <button
              className="button button--compact"
              type="button"
              disabled={busy !== null}
              onClick={resetAll}
            >
              Reset all site permissions
            </button>
          </div>
        </>
      )}
    </div>
  );
}

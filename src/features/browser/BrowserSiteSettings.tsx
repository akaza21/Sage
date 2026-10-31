import { useCallback, useEffect, useRef, useState } from "react";

import {
  browserSiteSettingReset,
  browserSiteSettingSet,
  browserSiteSettings,
  browserSiteSettingsResetAll,
  type BrowserSiteSetting,
} from "../../ports/nativeBrowserSiteSettingsPort";
import type { BrowserPermissionKind } from "../../ports/nativeBrowserPort";
import {
  admitBrowserSiteSettings,
  SITE_PERMISSION_LABELS,
} from "./browserSiteSettings";

const LOAD_FAILURE = "Site permissions could not be loaded.";
const SAVE_FAILURE = "The site permission could not be changed.";

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
    void import("@tauri-apps/api/event")
      .then(async ({ listen }) => {
        if (!mounted) return;
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
      })
      .catch(() => undefined);
    // A change event that lands first is newer than this initial read.
    browserSiteSettings().then(
      (loaded) => {
        if (!mounted || sawEvent) return;
        setRows(admitBrowserSiteSettings(loaded));
        setReady(true);
      },
      () => {
        if (mounted && !sawEvent) setError(LOAD_FAILURE);
      },
    );
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
      const stored = admitBrowserSiteSettings(await action());
      if (seen === events.current) setRows(stored);
    } catch {
      setError(SAVE_FAILURE);
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

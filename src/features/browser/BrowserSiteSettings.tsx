import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";

import { browserSiteSettingReset } from "../../ports/nativeBrowserPort";
import {
  browserSiteReloadTabs,
  browserSiteSettingSet,
  browserSiteSettings,
  browserSiteSettingsResetAll,
  type BrowserSiteSetting,
} from "../../ports/nativeBrowserSiteSettingsPort";
import type { BrowserPermissionKind } from "../../ports/nativeBrowserPort";
import { ConfirmDialog } from "../../ui/ConfirmDialog";
import {
  admitSiteSettingsView,
  SITE_PERMISSION_LABELS,
} from "./browserSiteSettingsModel";

const LOAD_FAILURE = "Site permissions could not be loaded.";
const SAVE_FAILURE = "The site permission could not be changed.";
const RELOAD_FAILURE = "The tabs could not be reloaded.";

const rowKey = (row: Pick<BrowserSiteSetting, "origin" | "kind">) =>
  `${row.origin} ${row.kind}`;

const DESCRIPTION_ID = "browser-site-permissions-description";

export function BrowserSitePermissionsGroup({
  children,
}: {
  children?: ReactNode;
}) {
  return (
    <section
      className="settings-surface__group"
      role="group"
      aria-labelledby="browser-site-permissions-heading"
    >
      <h3 id="browser-site-permissions-heading">Site permissions</h3>
      <p className="settings-surface__group-note" id={DESCRIPTION_ID}>
        Your answers to sites that asked for your camera, microphone, location,
        notifications, or clipboard. They apply to every browser tab and stay
        after Clear browsing data. Reset one to have the site ask again.
      </p>
      <BrowserSiteSettings describedBy={DESCRIPTION_ID} />
      {children}
    </section>
  );
}

export function BrowserSiteSettings({ describedBy }: { describedBy: string }) {
  const [rows, setRows] = useState<BrowserSiteSetting[]>([]);
  const [ready, setReady] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirmResetAll, setConfirmResetAll] = useState(false);
  // Sites of tabs whose page may still use an Allow that ended; Rust
  // tracks them until the page reloads or closes.
  const [staleSites, setStaleSites] = useState<string[]>([]);
  // Counts change events, so a reply that a newer event overtook is dropped.
  const events = useRef(0);

  const show = useCallback((input: unknown) => {
    const view = admitSiteSettingsView(input);
    setRows(view.settings);
    setStaleSites(view.staleSites);
    setReady(true);
  }, []);

  const retry = useCallback(async () => {
    const seen = events.current;
    try {
      const loaded = await browserSiteSettings();
      if (seen === events.current) show(loaded);
      setReady(true);
      setError(null);
    } catch {
      setError(LOAD_FAILURE);
    }
  }, [show]);

  useEffect(() => {
    let mounted = true;
    let unlisten: (() => void) | undefined;
    // Subscribe before the first read, so a change made in a browser tab
    // between the two is not lost. Only events after the read starts
    // invalidate its reply.
    void (async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        const dispose = await listen<unknown>(
          "browser:site-settings",
          (event) => {
            if (!mounted) return;
            events.current += 1;
            show(event.payload);
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
      const seen = events.current;
      try {
        const loaded = await browserSiteSettings();
        if (!mounted || seen !== events.current) return;
        show(loaded);
      } catch {
        if (mounted && seen === events.current) setError(LOAD_FAILURE);
      }
    })();
    return () => {
      mounted = false;
      unlisten?.();
    };
  }, [show]);

  async function run(
    key: string,
    action: () => Promise<unknown>,
    failure: string,
  ) {
    if (busy !== null) return;
    setBusy(key);
    setError(null);
    const seen = events.current;
    try {
      const view = await action();
      if (seen === events.current) show(view);
    } catch {
      setError(failure);
    } finally {
      setBusy(null);
    }
  }

  const change = (key: string, action: () => Promise<unknown>) =>
    run(key, action, SAVE_FAILURE);

  function resetAll() {
    setConfirmResetAll(false);
    void change("all", browserSiteSettingsResetAll);
  }

  const label = (kind: BrowserPermissionKind) => SITE_PERMISSION_LABELS[kind];

  return (
    <div className="settings-surface__site-permissions">
      {confirmResetAll ? (
        <ConfirmDialog
          title="Reset all site permissions?"
          description="This removes every saved Allow and Block decision. Sites will ask again, and Sage will try to reload pages that were allowed access."
          confirmLabel="Reset all site permissions"
          onConfirm={resetAll}
          onCancel={() => setConfirmResetAll(false)}
        />
      ) : null}
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
      {staleSites.length ? (
        <div className="settings-surface__error" role="alert">
          <span>
            Tabs on {staleSites.join(", ")} have not reloaded since an Allow
            ended and may still use it. Reload or close them.
          </span>
          <button
            className="button button--compact"
            type="button"
            disabled={busy !== null}
            onClick={() =>
              void run("reload", browserSiteReloadTabs, RELOAD_FAILURE)
            }
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
              onClick={() => setConfirmResetAll(true)}
            >
              Reset all site permissions
            </button>
          </div>
        </>
      )}
    </div>
  );
}

import { useEffect, useState } from "react";

import {
  browserPermissionResolve,
  browserPermissionStatus,
  type BrowserPermissionKind,
  type BrowserPermissionSnapshot,
} from "../../ports/nativeBrowserPort";
import {
  browserSiteReloadTabs,
  browserSiteSettingReset,
} from "../../ports/nativeBrowserSiteSettingsPort";
import { isTauriRuntime } from "../../ports/nativeInvoke";

const labels: Record<BrowserPermissionKind, string> = {
  camera: "camera",
  microphone: "microphone",
  geolocation: "location",
  notifications: "notifications",
  clipboardRead: "clipboard",
};

/** The strings in a site list from Rust; anything malformed is none. */
function sites(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((site): site is string => typeof site === "string")
    : [];
}

/** The sites a reset reports as not reloaded. */
function reloadFailedSites(update: unknown): string[] {
  return typeof update === "object" && update !== null
    ? sites((update as { reloadFailed?: unknown }).reloadFailed)
    : [];
}

export function BrowserSitePermissions({
  tabId,
  showEmpty = false,
}: {
  tabId: string;
  showEmpty?: boolean;
}) {
  const [snapshot, setSnapshot] = useState<BrowserPermissionSnapshot | null>(
    null,
  );
  const [busy, setBusy] = useState<number | null>(null);
  const [revokeBusy, setRevokeBusy] = useState<string | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Sites whose tabs did not reload after a revoke; their pages may keep
  // access until they reload.
  const [unreloaded, setUnreloaded] = useState<string[]>([]);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    let mounted = true;
    let sawEvent = false;
    let unlisten: (() => void) | undefined;
    void import("@tauri-apps/api/event")
      .then(async ({ listen }) => {
        if (!mounted) return;
        unlisten = await listen<BrowserPermissionSnapshot>(
          "browser:permissions",
          (event) => {
            if (mounted && event.payload.tabId === tabId) {
              sawEvent = true;
              setSnapshot(event.payload);
            }
          },
        );
        if (!mounted) {
          unlisten();
          return;
        }
        const initial = await browserPermissionStatus(tabId);
        if (mounted && !sawEvent) setSnapshot(initial);
      })
      .catch((cause) => {
        if (mounted) setError(String(cause).slice(0, 512));
      });
    return () => {
      mounted = false;
      unlisten?.();
    };
  }, [tabId]);

  async function decide(id: number, allow: boolean) {
    setBusy(id);
    setError(null);
    try {
      await browserPermissionResolve(tabId, id, allow);
      setSnapshot((current) =>
        current
          ? {
              ...current,
              pending: current.pending.filter((item) => item.id !== id),
            }
          : current,
      );
    } catch (cause) {
      setError(String(cause).slice(0, 512));
    } finally {
      setBusy(null);
    }
  }

  async function revoke(origin: string, kind: BrowserPermissionKind) {
    setRevokeBusy(`${origin}:${kind}`);
    setError(null);
    try {
      const failed = reloadFailedSites(
        await browserSiteSettingReset(origin, kind),
      );
      if (failed.length) {
        setUnreloaded((current) => [...new Set([...current, ...failed])]);
      }
      setSnapshot((current) =>
        current
          ? {
              ...current,
              decisions: current.decisions.filter(
                (item) => item.origin !== origin || item.kind !== kind,
              ),
            }
          : current,
      );
    } catch (cause) {
      setError(String(cause).slice(0, 512));
    } finally {
      setRevokeBusy(null);
    }
  }

  async function reloadAgain() {
    setRevokeBusy("reload");
    setError(null);
    try {
      setUnreloaded(sites(await browserSiteReloadTabs(unreloaded)));
    } catch (cause) {
      setError(String(cause).slice(0, 512));
    } finally {
      setRevokeBusy(null);
    }
  }

  if (
    !showEmpty &&
    !unreloaded.length &&
    !snapshot?.pending.length &&
    !snapshot?.decisions.length &&
    !error
  )
    return null;
  return (
    <aside className="browser-site-permissions" aria-label="Site permissions">
      {snapshot?.pending.map((prompt) => (
        <div className="browser-site-permissions__prompt" key={prompt.id}>
          <span>
            {prompt.origin} wants to use your {labels[prompt.kind]}.
          </span>
          <button
            type="button"
            disabled={busy !== null}
            onClick={() => void decide(prompt.id, false)}
          >
            Block {labels[prompt.kind]}
          </button>
          <button
            type="button"
            disabled={busy !== null}
            onClick={() => void decide(prompt.id, true)}
          >
            Allow {labels[prompt.kind]}
          </button>
        </div>
      ))}
      {snapshot?.decisions.length ? (
        <>
          <button
            type="button"
            aria-expanded={expanded}
            onClick={() => setExpanded((value) => !value)}
          >
            Site permissions
          </button>
          {expanded || showEmpty ? (
            <>
              <p>
                Saved for this site in every tab. Revoking an allowed permission
                reloads the site to stop active access.
              </p>
              <ul>
                {snapshot.decisions.map((decision) => (
                  <li key={`${decision.origin}:${decision.kind}`}>
                    <span>
                      {decision.origin} — {labels[decision.kind]}:{" "}
                      {decision.decision === "allow" ? "Allowed" : "Blocked"}
                    </span>
                    <button
                      type="button"
                      aria-label={`Revoke ${labels[decision.kind]} for ${decision.origin}`}
                      disabled={revokeBusy !== null}
                      onClick={() =>
                        void revoke(decision.origin, decision.kind)
                      }
                    >
                      Revoke
                    </button>
                  </li>
                ))}
              </ul>
            </>
          ) : null}
        </>
      ) : showEmpty ? (
        <p>No site permissions saved for this site.</p>
      ) : null}
      {unreloaded.length ? (
        <p role="alert">
          Revoked, but tabs on {unreloaded.join(", ")} could not reload and may
          keep access until they do.{" "}
          <button
            type="button"
            disabled={revokeBusy !== null}
            onClick={() => void reloadAgain()}
          >
            Reload tabs
          </button>
        </p>
      ) : null}
      {error ? <p role="alert">{error}</p> : null}
    </aside>
  );
}

export function BrowserDrawers({
  tabId,
  settingsOpen,
  historyOpen,
  historyEntries,
  onCloseSettings,
  onCloseHistory,
  onNavigate,
}: {
  tabId: string;
  settingsOpen: boolean;
  historyOpen: boolean;
  historyEntries: readonly string[];
  onCloseSettings: () => void;
  onCloseHistory: () => void;
  onNavigate: (url: string) => void;
}) {
  return (
    <>
      {settingsOpen ? (
        <aside className="browser-surface__drawer" aria-label="Site settings">
          <button type="button" onClick={onCloseSettings}>
            Close site settings
          </button>
          <BrowserSitePermissions tabId={tabId} showEmpty />
        </aside>
      ) : (
        <BrowserSitePermissions tabId={tabId} />
      )}
      {historyOpen ? (
        <aside className="browser-surface__drawer" aria-label="Browser history">
          <button type="button" onClick={onCloseHistory}>
            Close history
          </button>
          <h2>History</h2>
          {historyEntries.length ? (
            <ul>
              {historyEntries.map((url) => (
                <li key={url}>
                  <button type="button" onClick={() => onNavigate(url)}>
                    {url}
                  </button>
                </li>
              ))}
            </ul>
          ) : (
            <p>No history for this tab yet.</p>
          )}
        </aside>
      ) : null}
    </>
  );
}

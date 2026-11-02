import { useEffect, useState } from "react";

import {
  browserPermissionResolve,
  browserPermissionStatus,
  browserSiteSettingReset,
  type BrowserPermissionKind,
  type BrowserPermissionSnapshot,
} from "../../ports/nativeBrowserPort";
import { isTauriRuntime } from "../../ports/nativeInvoke";

const labels: Record<BrowserPermissionKind, string> = {
  camera: "camera",
  microphone: "microphone",
  geolocation: "location",
  notifications: "notifications",
  clipboardRead: "clipboard",
};

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
      // Rust sends every tab's fresh snapshot once the reset is saved.
      await browserSiteSettingReset(origin, kind);
    } catch (cause) {
      setError(String(cause).slice(0, 512));
    } finally {
      setRevokeBusy(null);
    }
  }

  if (
    !showEmpty &&
    !snapshot?.staleAccess &&
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
              <p>Saved for this site. Revoking an Allow reloads the site.</p>
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
        <p>No site permissions saved for this tab.</p>
      ) : null}
      {snapshot?.staleAccess ? (
        <p role="alert">Reload this page to end revoked access.</p>
      ) : null}
      {error ? <p role="alert">{error}</p> : null}
    </aside>
  );
}

export function BrowserDrawers({
  tabId,
  settingsOpen,
  onCloseSettings,
}: {
  tabId: string;
  settingsOpen: boolean;
  onCloseSettings: () => void;
}) {
  return settingsOpen ? (
    <aside className="browser-surface__drawer" aria-label="Site settings">
      <button type="button" onClick={onCloseSettings}>
        Close site settings
      </button>
      <BrowserSitePermissions tabId={tabId} showEmpty />
    </aside>
  ) : (
    <BrowserSitePermissions tabId={tabId} />
  );
}

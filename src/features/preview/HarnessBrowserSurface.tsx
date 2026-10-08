import {
  ChevronLeft,
  MoreHorizontal,
  RotateCw,
  ChevronRight,
  X,
} from "../../ui/leanIcons";

import {
  useId,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent,
  type ReactNode,
} from "react";

import {
  applyBrowseIntent,
  browseBack,
  browseForward,
  browseHistoryForOwner,
  clearBrowseSession,
} from "./harnessBrowse";
import { useHarnessBrowseSession } from "./useHarnessBrowseSession";
import type { HarnessBrowseOwner } from "./harnessBrowse";
import { MenuPopover } from "../../ui/MenuPopover";
import { ResourceStateNotice } from "../../ui/ResourceStateNotice";

export function HarnessBrowserSurface({
  owner,
  replacement,
}: {
  owner: HarnessBrowseOwner | null;
  replacement?: ReactNode;
}) {
  const session = useHarnessBrowseSession(owner);
  const history = owner
    ? browseHistoryForOwner(owner)
    : { canBack: false, canForward: false };
  const sessionHref = session?.href ?? "";
  const [override, setOverride] = useState<{
    href: string;
    draft: string;
  } | null>(null);
  const [denied, setDenied] = useState(false);
  const [frameEpoch, setFrameEpoch] = useState(0);
  const [controlsOpen, setControlsOpen] = useState(false);
  const addressRef = useRef<HTMLInputElement | null>(null);
  const browserRef = useRef<HTMLElement | null>(null);
  const controlsTriggerRef = useRef<HTMLButtonElement | null>(null);
  const controlsMenuId = useId();
  const draft =
    override && override.href === sessionHref ? override.draft : sessionHref;

  const submitAddress = (event: FormEvent) => {
    event.preventDefault();
    if (!owner) return;
    const next = applyBrowseIntent({ owner, url: draft.trim() });
    if (!next) {
      setDenied(true);
      return;
    }
    setDenied(false);
    setOverride(null);
  };

  const closeBrowser = () => {
    if (!owner) return;
    clearBrowseSession(owner);
    addressRef.current?.focus();
  };

  const returnToPreviewTab = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    browserRef.current
      ?.closest<HTMLElement>(".agent-artifact-pane")
      ?.querySelector<HTMLButtonElement>('[role="tab"][aria-label="Preview"]')
      ?.focus();
  };

  return (
    <section
      ref={browserRef}
      className="harness-browser"
      aria-label="Sage browser"
    >
      <header className="harness-browser__chrome">
        <span
          className={`harness-browser__connection harness-browser__connection--${
            session ? "active" : "idle"
          }`}
          aria-label={
            session ? "Loopback preview active" : "No preview connected"
          }
          title={session ? "Loopback preview active" : "No preview connected"}
          role="status"
        >
          <span aria-hidden="true" />
          <span className="harness-browser__connection-label">
            {session ? "Local" : "Idle"}
          </span>
        </span>
        <div
          className="harness-browser__controls"
          aria-label="Preview controls"
        >
          <button
            type="button"
            aria-label="Back"
            title="Back"
            disabled={!history.canBack || !owner}
            onClick={() => {
              if (owner) browseBack(owner);
            }}
          >
            <ChevronLeft size={14} aria-hidden="true" />
          </button>
          <button
            type="button"
            aria-label="Forward"
            title="Forward"
            disabled={!history.canForward || !owner}
            onClick={() => {
              if (owner) browseForward(owner);
            }}
          >
            <ChevronRight size={14} aria-hidden="true" />
          </button>
          <button
            type="button"
            aria-label="Reload preview"
            title="Reload preview"
            disabled={!session}
            onClick={() => setFrameEpoch((epoch) => epoch + 1)}
          >
            <RotateCw size={14} aria-hidden="true" />
          </button>
          {session && owner ? (
            <button
              type="button"
              aria-label="Close browser"
              title="Close browser"
              onClick={closeBrowser}
            >
              <X size={14} aria-hidden="true" />
            </button>
          ) : null}
        </div>
        <button
          ref={controlsTriggerRef}
          className="harness-browser__controls-trigger"
          type="button"
          aria-label="Preview controls"
          aria-controls={controlsMenuId}
          aria-expanded={controlsOpen}
          title="Preview controls"
          onClick={() => setControlsOpen((open) => !open)}
        >
          <MoreHorizontal size={16} aria-hidden="true" />
        </button>
        <form className="harness-browser__address" onSubmit={submitAddress}>
          <input
            ref={addressRef}
            aria-label="Preview address"
            aria-invalid={denied || undefined}
            spellCheck={false}
            value={draft}
            placeholder="http://127.0.0.1:port/"
            disabled={!owner}
            onKeyDown={returnToPreviewTab}
            onChange={(event) => {
              setOverride({
                href: sessionHref,
                draft: event.currentTarget.value,
              });
              setDenied(false);
            }}
          />
        </form>
      </header>
      <MenuPopover
        ariaLabel="Preview controls"
        id={controlsMenuId}
        open={controlsOpen}
        onDismiss={() => setControlsOpen(false)}
        triggerRef={controlsTriggerRef}
      >
        <button
          type="button"
          role="menuitem"
          className="menu-item"
          aria-label="Back"
          disabled={!history.canBack || !owner}
          onClick={() => {
            if (owner) browseBack(owner);
            setControlsOpen(false);
          }}
        >
          Back
        </button>
        <button
          type="button"
          role="menuitem"
          className="menu-item"
          aria-label="Forward"
          disabled={!history.canForward || !owner}
          onClick={() => {
            if (owner) browseForward(owner);
            setControlsOpen(false);
          }}
        >
          Forward
        </button>
        <button
          type="button"
          role="menuitem"
          className="menu-item"
          aria-label="Reload preview"
          disabled={!session}
          onClick={() => {
            setFrameEpoch((epoch) => epoch + 1);
            setControlsOpen(false);
          }}
        >
          Reload preview
        </button>
        {session && owner ? (
          <button
            type="button"
            role="menuitem"
            className="menu-item"
            aria-label="Close browser"
            onClick={() => {
              closeBrowser();
              setControlsOpen(false);
            }}
          >
            Close browser
          </button>
        ) : null}
      </MenuPopover>
      {denied ? (
        <p className="harness-browser__denied" role="alert">
          Only loopback HTTP(S) with a port
        </p>
      ) : null}
      {replacement ??
        (session ? (
          <iframe
            key={`${session.href}:${frameEpoch}`}
            className="harness-browser__frame"
            title="Sage browser"
            src={session.href}
            sandbox="allow-scripts allow-same-origin allow-forms"
            referrerPolicy="no-referrer"
          />
        ) : owner ? (
          <ResourceStateNotice
            state="idle"
            title="Preview is idle"
            detail="Preview shows loopback development servers started from this Chat. Only loopback HTTP(S) with a port; public sites stay closed."
          />
        ) : (
          <ResourceStateNotice
            state="idle"
            title="Preview unavailable"
            detail="Select a Chat to preview loopback development servers."
          />
        ))}
    </section>
  );
}

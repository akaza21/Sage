import { ChevronLeft, ChevronRight, RotateCw, X } from "lucide-react";
import { useRef, useState, type FormEvent } from "react";

import {
  applyBrowseIntent,
  browseBack,
  browseForward,
  browseHistoryForOwner,
  clearBrowseSession,
} from "./harnessBrowse";
import { useHarnessBrowseSession } from "./useHarnessBrowseSession";
import type { HarnessBrowseOwner } from "./harnessBrowse";
import { ResourceStateNotice } from "../../ui/ResourceStateNotice";

export function HarnessBrowserSurface({
  owner,
}: {
  owner: HarnessBrowseOwner | null;
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
  const addressRef = useRef<HTMLInputElement | null>(null);
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

  return (
    <section className="harness-browser" aria-label="Sage browser">
      <header className="harness-browser__chrome">
        <button
          type="button"
          aria-label="Back"
          title="Back"
          disabled={!history.canBack || !owner}
          onClick={() => {
            if (owner) browseBack(owner);
          }}
        >
          <ChevronLeft size={13} aria-hidden="true" />
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
          <ChevronRight size={13} aria-hidden="true" />
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
            onChange={(event) => {
              setOverride({
                href: sessionHref,
                draft: event.currentTarget.value,
              });
              setDenied(false);
            }}
          />
        </form>
        <button
          type="button"
          aria-label="Reload preview"
          title="Reload preview"
          disabled={!session}
          onClick={() => setFrameEpoch((epoch) => epoch + 1)}
        >
          <RotateCw size={13} aria-hidden="true" />
        </button>
        {session && owner ? (
          <button
            type="button"
            aria-label="Close browser"
            title="Close browser"
            onClick={() => {
              clearBrowseSession(owner);
              addressRef.current?.focus();
            }}
          >
            <X size={13} aria-hidden="true" />
          </button>
        ) : null}
      </header>
      {denied ? (
        <p className="harness-browser__denied" role="alert">
          Only loopback HTTP(S) with a port
        </p>
      ) : null}
      {session ? (
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
      )}
    </section>
  );
}

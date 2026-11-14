import { lazy, Suspense, useCallback, useEffect, useState } from "react";
import { createPortal } from "react-dom";
import {
  browserControl,
  type BrowserAnnotateSelection,
  type BrowserControlAction,
  type BrowserNavigation,
} from "../../ports/nativeBrowserPort";
import { registerBrowserElementSink } from "../../app/browserAnnotations";
import { browserElementChatSink } from "../../app/browserElementChatSink";
import { subscribeBrowserTab } from "./browserTabEvents";
const BrowserAnnotateComment = lazy(() => import("./BrowserAnnotatePopover"));

// Stateless: it reads the pinned Agent Chat at each pick, so one
// registration serves every browser tab.
registerBrowserElementSink(browserElementChatSink);

export function BrowserAnnotateControl({
  navigation,
  onControl,
  onError,
}: {
  navigation: BrowserNavigation | null;
  onControl: (action: BrowserControlAction) => void;
  onError: (message: string) => void;
}) {
  const tabId = navigation?.tabId;
  const [frame, setFrame] = useState<Element | null>(null);
  const findFrame = useCallback(
    (button: HTMLButtonElement | null) =>
      setFrame(
        button
          ?.closest(".browser-surface")
          ?.querySelector(".browser-surface__viewport") ?? null,
      ),
    [],
  );
  const [active, setActive] = useState(false);
  const [selection, setSelection] = useState<BrowserAnnotateSelection | null>(
    null,
  );
  const set = (on: boolean) => onControl(on ? "annotateOn" : "annotateOff");
  useEffect(() => {
    if (!tabId) return;
    const stop = subscribeBrowserTab(
      "browser:annotate",
      tabId,
      (payload) => {
        setActive(payload.active);
        setSelection(payload.selection ?? null);
        if (payload.error) onError(payload.error);
      },
      // A new chrome has no comment box: drop any element still waiting,
      // which shows the page again.
      async () => {
        await browserControl(tabId, "annotateCancel").catch(() => undefined);
      },
    );
    return () => {
      stop();
      setActive(false);
      setSelection(null);
    };
  }, [tabId, onError]);
  const reason = navigation?.annotateUnavailableReason;
  return (
    <>
      <button
        ref={findFrame}
        type="button"
        className="browser-surface__annotate"
        aria-pressed={active}
        disabled={!navigation || Boolean(reason)}
        title={reason ?? undefined}
        onClick={() => set(!active)}
        onKeyDown={(event) => {
          if (event.key !== "Escape" || !active) return;
          event.preventDefault();
          event.stopPropagation();
          if (selection) onControl("annotateCancel");
          else set(false);
        }}
      >
        Annotate
      </button>
      {tabId && selection && frame
        ? createPortal(
            <Suspense fallback={null}>
              <BrowserAnnotateComment tabId={tabId} selection={selection} />
            </Suspense>,
            frame,
          )
        : null}
    </>
  );
}

import { useCallback, useEffect, useState } from "react";
import { createPortal } from "react-dom";
import {
  browserControl,
  type BrowserAnnotateChange,
  type BrowserAnnotateSelection,
  type BrowserControlAction,
  type BrowserNavigation,
} from "../../ports/nativeBrowserPort";
import BrowserAnnotateComment from "./BrowserAnnotatePopover";

/**
 * Rust owns the mode, the picked element, the disabled reason, and Escape in
 * the page. While an element waits for a comment the comment box covers the
 * page frame. Escape everywhere drops a waiting element first, and only then
 * turns Annotate off.
 */
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
          ?.querySelector(".browser-surface__frame") ?? null,
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
    const unlisten = import("@tauri-apps/api/event")
      .then(({ listen }) =>
        listen<BrowserAnnotateChange>("browser:annotate", ({ payload }) => {
          if (payload.tabId !== tabId) return;
          setActive(payload.active);
          setSelection(payload.selection ?? null);
          if (payload.error) onError(payload.error);
        }),
      )
      // A new chrome has no comment box: drop any element still waiting,
      // which shows the page again.
      .then((stop) => {
        void browserControl(tabId, "annotateCancel").catch(() => undefined);
        return stop;
      })
      .catch(() => () => undefined);
    return () => {
      void unlisten.then((stop) => stop());
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
        onKeyDown={(event) =>
          event.key === "Escape" &&
          active &&
          (selection ? onControl("annotateCancel") : set(false))
        }
      >
        Annotate
      </button>
      {tabId && selection && frame
        ? createPortal(
            <BrowserAnnotateComment tabId={tabId} selection={selection} />,
            frame,
          )
        : null}
    </>
  );
}

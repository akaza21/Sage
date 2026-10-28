import { useEffect, useState } from "react";
import type {
  BrowserControlAction,
  BrowserNavigation,
} from "../../ports/nativeBrowserPort";

/** Rust owns the mode, the disabled reason, and Escape in the page. */
export function BrowserAnnotateControl({
  navigation,
  onControl,
}: {
  navigation: BrowserNavigation | null;
  onControl: (action: BrowserControlAction) => void;
}) {
  const tabId = navigation?.tabId;
  const [active, setActive] = useState(false);
  const set = (on: boolean) => onControl(on ? "annotateOn" : "annotateOff");
  useEffect(() => {
    if (!tabId) return;
    const unlisten = import("@tauri-apps/api/event")
      .then(({ listen }) =>
        listen<{ tabId: string; active: boolean }>(
          "browser:annotate",
          ({ payload }) => payload.tabId === tabId && setActive(payload.active),
        ),
      )
      .catch(() => () => undefined);
    return () => {
      void unlisten.then((stop) => stop());
      setActive(false);
    };
  }, [tabId]);
  const reason = navigation?.annotateUnavailableReason;
  return (
    <button
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
        set(false);
      }}
    >
      Annotate
    </button>
  );
}

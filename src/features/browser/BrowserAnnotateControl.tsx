import { useEffect, useState } from "react";
import type { BrowserNavigation } from "../../ports/nativeBrowserPort";
import { invokeNative, isTauriRuntime } from "../../ports/nativeInvoke";
import { leanIcon } from "../../ui/leanIconCore";

const AnnotateIcon = leanIcon("lucide lucide-square-dashed-mouse-pointer", [
  [
    "path",
    {
      d: "M12.034 12.681a.498.498 0 0 1 .647-.647l9 3.5a.5.5 0 0 1-.033.943l-3.444 1.068a1 1 0 0 0-.66.66l-1.067 3.443a.5.5 0 0 1-.943.033z",
    },
  ],
  ["path", { d: "M5 3a2 2 0 0 0-2 2" }],
  ["path", { d: "M19 3a2 2 0 0 1 2 2" }],
  ["path", { d: "M5 21a2 2 0 0 1-2-2" }],
  ["path", { d: "M9 3h1M9 21h2M14 3h1M3 9v1M21 9v2M3 14v1" }],
]);

/** Rust reports why Annotate is unavailable on this platform, if it is. */
export type AnnotatableNavigation = BrowserNavigation & {
  annotateUnavailableReason?: string | null;
};

interface AnnotateChange {
  tabId: string;
  active: boolean;
}

function setAnnotate(
  tabId: string,
  on: boolean,
  onError?: (message: string) => void,
) {
  void invokeNative("browser_control", {
    tabId,
    action: on ? "annotateOn" : "annotateOff",
  }).catch((cause) => onError?.(String(cause).slice(0, 512)));
}

/**
 * The Annotate pill. Rust owns whether the tab is annotating and announces
 * every change, including the exits it makes itself on navigation, hide and
 * close; Escape anywhere in Sage's UI leaves too.
 */
export function BrowserAnnotateControl({
  navigation,
  ready,
  onError,
}: {
  navigation: AnnotatableNavigation | null;
  ready: boolean;
  onError?: (message: string) => void;
}) {
  const tabId = navigation?.tabId;
  const [active, setActive] = useState(false);
  useEffect(() => {
    if (!tabId || !isTauriRuntime()) return;
    let live = true;
    let unlisten: (() => void) | undefined;
    void import("@tauri-apps/api/event")
      .then(({ listen }) =>
        listen<AnnotateChange>("browser:annotate", ({ payload }) => {
          if (payload.tabId === tabId) setActive(payload.active);
        }),
      )
      .then((stop) => {
        if (live) unlisten = stop;
        else stop();
      })
      .catch(() => undefined);
    return () => {
      live = false;
      unlisten?.();
      setActive(false);
    };
  }, [tabId]);
  useEffect(() => {
    if (!active || !tabId) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !event.defaultPrevented)
        setAnnotate(tabId, false, onError);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [active, tabId, onError]);
  const reason =
    navigation?.annotateUnavailableReason ??
    (ready && tabId ? null : "Open a page to annotate");
  return (
    <button
      type="button"
      className="browser-surface__annotate"
      aria-pressed={active}
      disabled={Boolean(reason)}
      title={reason ?? (active ? "Stop annotating (Esc)" : "Annotate the page")}
      onClick={() => tabId && setAnnotate(tabId, !active, onError)}
    >
      <AnnotateIcon size={16} aria-hidden="true" />
      Annotate
    </button>
  );
}

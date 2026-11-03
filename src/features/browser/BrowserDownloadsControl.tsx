import { Download } from "../../ui/leanIcons";

import {
  lazy,
  Suspense,
  useEffect,
  useRef,
  type CSSProperties,
  type KeyboardEvent,
} from "react";
import { useDismissOnOutsidePointer } from "./browserPopover";
import { useNativePageLayer } from "../../ui/nativePageLayers";
import {
  activeDownloadProgress,
  isActiveDownload,
  useBrowserDownloads,
} from "./useBrowserDownloads";

const BrowserDownloadsPanel = lazy(() => import("./BrowserDownloadsPanel"));

/**
 * The toolbar's ⬇ button and its popover. A ring on
 * the button shows the active downloads' progress while the popover is shut.
 */
export function BrowserDownloadsControl({
  tabId,
  open,
  onOpenChange,
}: {
  tabId: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const downloads = useBrowserDownloads(tabId);
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const popover = useRef<HTMLDivElement>(null);
  useNativePageLayer(popover, { open });
  const active = downloads.filter(isActiveDownload).length;
  const progress = activeDownloadProgress(downloads);
  useDismissOnOutsidePointer(open, [root], onOpenChange);
  useEffect(() => {
    if (open) popover.current?.focus();
  }, [open]);
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!open || event.key !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    onOpenChange(false);
    trigger.current?.focus();
  };
  return (
    <div
      ref={root}
      className="browser-downloads"
      data-active={active > 0 || undefined}
      data-indeterminate={(active > 0 && progress === null) || undefined}
      onKeyDown={onKeyDown}
    >
      <button
        ref={trigger}
        type="button"
        className="browser-downloads__trigger"
        aria-label={active ? `Downloads, ${active} active` : "Downloads"}
        aria-haspopup="dialog"
        aria-expanded={open}
        style={
          progress === null
            ? undefined
            : ({
                "--browser-download-progress": `${Math.round(progress * 100)}%`,
              } as CSSProperties)
        }
        onClick={() => onOpenChange(!open)}
      >
        <Download size={16} aria-hidden="true" />
      </button>
      {open ? (
        <div
          ref={popover}
          className="browser-downloads__popover"
          role="dialog"
          aria-label="Downloads"
          tabIndex={-1}
        >
          <Suspense fallback={<p role="status">Loading downloads…</p>}>
            <BrowserDownloadsPanel tabId={tabId} downloads={downloads} />
          </Suspense>
        </div>
      ) : null}
    </div>
  );
}

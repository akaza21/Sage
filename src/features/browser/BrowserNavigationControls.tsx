import {
  ArrowLeft,
  ArrowRight,
  Download,
  Ellipsis,
  RotateCw,
  X,
} from "lucide-react";
import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type MouseEvent,
  type ReactNode,
  type RefObject,
} from "react";

import type { BrowserNavigation } from "../../ports/nativeBrowserPort";
import { useWorkbenchStore } from "../../app/store";
import type { BrowserPageIdentity } from "./browserAddress";

interface Props {
  navigation: BrowserNavigation | null;
  page: BrowserPageIdentity | null;
  address: string;
  addressField: RefObject<HTMLInputElement | null>;
  onAddressChange: (value: string) => void;
  onNavigate: () => void;
  onControl: (action: "back" | "forward" | "reload" | "stop") => void;
  onFind: () => void;
  onZoom: (delta: number) => void;
  onZoomReset: () => void;
  onDownloads: () => void;
  onHistory: () => void;
  onSettings: () => void;
  onCopy: () => void;
  onExternal: () => void;
  clearDataControl: ReactNode;
  pageControls?: ReactNode;
}

export function BrowserNavigationControls({
  navigation,
  page,
  address,
  addressField,
  onAddressChange,
  onNavigate,
  onControl,
  onFind,
  onZoom,
  onZoomReset,
  onDownloads,
  onHistory,
  onSettings,
  onCopy,
  onExternal,
  clearDataControl,
  pageControls,
}: Props) {
  const [menuOpen, setMenuOpen] = useState(false);
  const [editing, setEditing] = useState(false);
  const menuTrigger = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (editing) addressField.current?.select();
  }, [editing, addressField]);
  useEffect(() => {
    if (menuOpen)
      menu.current
        ?.querySelector<HTMLButtonElement>('[role="menuitem"]:not(:disabled)')
        ?.focus();
  }, [menuOpen]);
  const run = (action: () => void) => {
    setMenuOpen(false);
    menuTrigger.current?.focus();
    action();
  };
  const onMenuClickCapture = (event: MouseEvent<HTMLDivElement>) => {
    if (!(event.target instanceof Element)) return;
    if (!event.target.closest(".browser-clear-data--menu button")) return;
    setMenuOpen(false);
    menuTrigger.current?.focus();
  };
  const onMenuKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!menuOpen) return;
    if (event.key === "Escape") {
      event.preventDefault();
      setMenuOpen(false);
      menuTrigger.current?.focus();
      return;
    }
    if (!["ArrowUp", "ArrowDown", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    const items = Array.from(
      menu.current?.querySelectorAll<HTMLButtonElement>(
        '[role="menuitem"]:not(:disabled)',
      ) ?? [],
    );
    if (!items.length) return;
    const current = items.indexOf(document.activeElement as HTMLButtonElement);
    const next =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? items.length - 1
          : event.key === "ArrowDown"
            ? (current + 1) % items.length
            : (current - 1 + items.length) % items.length;
    items[next]?.focus();
  };
  return (
    <>
      <div className="browser-surface__nav-group">
        <button
          type="button"
          aria-label="Back"
          disabled={!navigation?.canGoBack}
          onClick={() => onControl("back")}
        >
          <ArrowLeft size={16} aria-hidden="true" />
        </button>
        <button
          type="button"
          aria-label="Forward"
          disabled={!navigation?.canGoForward}
          onClick={() => onControl("forward")}
        >
          <ArrowRight size={16} aria-hidden="true" />
        </button>
        <button
          type="button"
          aria-label={navigation?.loading ? "Stop loading" : "Reload"}
          disabled={!navigation?.url && !navigation?.loading}
          onClick={() => onControl(navigation?.loading ? "stop" : "reload")}
        >
          {navigation?.loading ? (
            <X size={16} aria-hidden="true" />
          ) : (
            <RotateCw size={16} aria-hidden="true" />
          )}
        </button>
      </div>
      <form
        className="browser-surface__address"
        onSubmit={(event) => {
          event.preventDefault();
          onNavigate();
        }}
      >
        <span
          className="browser-surface__address-security"
          aria-label={
            page
              ? page.transport === "secure"
                ? "Secure connection"
                : "Not secure"
              : "No page"
          }
        >
          {page?.transport === "secure" ? "⌁" : ""}
        </span>
        <input
          ref={addressField}
          aria-label="Address or search"
          placeholder="Search or enter a URL"
          type="text"
          maxLength={2048}
          value={editing || !page ? address : new URL(page.url).host}
          onFocus={() => setEditing(true)}
          onBlur={() => setEditing(false)}
          onChange={(event) => onAddressChange(event.target.value)}
        />
      </form>
      <div className="browser-surface__right-group" onKeyDown={onMenuKeyDown}>
        {pageControls}
        <button type="button" aria-label="Downloads" onClick={onDownloads}>
          <Download size={16} aria-hidden="true" />
        </button>
        <button
          ref={menuTrigger}
          type="button"
          aria-label="More options"
          aria-expanded={menuOpen}
          onClick={() => setMenuOpen((value) => !value)}
        >
          <Ellipsis size={18} aria-hidden="true" />
        </button>
        {menuOpen ? (
          <div
            ref={menu}
            className="browser-surface__menu"
            role="menu"
            aria-label="Browser menu"
            onClickCapture={onMenuClickCapture}
          >
            {page ? (
              <div className="browser-surface__menu-group">
                <button
                  role="menuitem"
                  type="button"
                  onClick={() => run(onFind)}
                >
                  Find in page
                </button>
                <div className="browser-surface__menu-zoom">
                  <span>Zoom</span>
                  <button
                    type="button"
                    role="menuitem"
                    aria-label="Zoom out"
                    disabled={(navigation?.zoomPercent ?? 100) <= 50}
                    onClick={() => onZoom(-25)}
                  >
                    −
                  </button>
                  <span>{navigation?.zoomPercent ?? 100}%</span>
                  <button
                    type="button"
                    role="menuitem"
                    aria-label="Zoom in"
                    disabled={(navigation?.zoomPercent ?? 100) >= 300}
                    onClick={() => onZoom(25)}
                  >
                    +
                  </button>
                  <button
                    type="button"
                    role="menuitem"
                    aria-label="Reset zoom"
                    onClick={onZoomReset}
                  >
                    ↺
                  </button>
                </div>
              </div>
            ) : null}
            <div className="browser-surface__menu-group">
              <button
                role="menuitem"
                type="button"
                onClick={() => run(onDownloads)}
              >
                Downloads
              </button>
              <button
                role="menuitem"
                type="button"
                onClick={() => run(onHistory)}
              >
                History
              </button>
              {clearDataControl}
            </div>
            <div className="browser-surface__menu-group">
              <button
                role="menuitem"
                type="button"
                onClick={() => run(onSettings)}
              >
                Browser settings
              </button>
              {page ? (
                <>
                  <button
                    role="menuitem"
                    type="button"
                    onClick={() => run(onCopy)}
                  >
                    Copy URL
                  </button>
                  <button
                    role="menuitem"
                    type="button"
                    onClick={() => run(onExternal)}
                  >
                    Open externally
                  </button>
                </>
              ) : null}
            </div>
          </div>
        ) : null}
      </div>
    </>
  );
}

export function BrowserFindControls({
  findField,
  findText,
  onFindTextChange,
  onFind,
  onClose,
}: {
  findField: RefObject<HTMLInputElement | null>;
  findText: string;
  onFindTextChange: (value: string) => void;
  onFind: (action: "start" | "previous" | "next") => void;
  onClose: () => void;
}) {
  return (
    <form
      className="browser-surface__find"
      onSubmit={(event) => {
        event.preventDefault();
        onFind("start");
      }}
    >
      <input
        ref={findField}
        aria-label="Find text"
        value={findText}
        maxLength={256}
        onChange={(event) => onFindTextChange(event.target.value)}
      />
      <button type="submit" disabled={!findText}>
        Find
      </button>
      <button
        type="button"
        aria-label="Previous match"
        onClick={() => onFind("previous")}
      >
        ↑
      </button>
      <button
        type="button"
        aria-label="Next match"
        onClick={() => onFind("next")}
      >
        ↓
      </button>
      <button type="button" aria-label="Close find" onClick={onClose}>
        <X size={16} aria-hidden="true" />
      </button>
    </form>
  );
}

export function ClosedBrowserSurface({ tabId }: { tabId: string }) {
  const openBrowser = useWorkbenchStore((state) => state.openBrowser);
  const closeTab = useWorkbenchStore((state) => state.closeTab);
  return (
    <section
      aria-label="Browser page"
      className="browser-surface browser-surface--closed"
    >
      <p>
        This browser session closed when Sage restarted. Its page was not
        restored.
      </p>
      <button type="button" onClick={() => void openBrowser()}>
        Open new browser
      </button>
      <button type="button" onClick={() => closeTab(tabId)}>
        Close tab
      </button>
    </section>
  );
}

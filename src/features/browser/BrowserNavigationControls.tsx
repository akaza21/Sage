import {
  ArrowLeft,
  ArrowRight,
  Ellipsis,
  Lock,
  RotateCw,
  X,
} from "../../ui/leanIcons";

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

import type {
  BrowserControlAction,
  BrowserNavigation,
} from "../../ports/nativeBrowserPort";
import { useAddressText, type BrowserPageIdentity } from "./browserAddressText";
import { BrowserAnnotateControl } from "./BrowserAnnotateControl";
import { BrowserSettingsMenuItems } from "./BrowserSettingsMenuItems";
import {
  browserMenuKeyDown,
  useDismissOnOutsidePointer,
} from "./browserPopover";
import { useToolbarOverflow } from "./useToolbarOverflow";
import { useNativePageLayer } from "../../ui/nativePageLayers";

interface Props {
  navigation: BrowserNavigation | null;
  page: BrowserPageIdentity | null;
  address: string;
  addressField: RefObject<HTMLInputElement | null>;
  onAddressChange: (value: string) => void;
  onNavigate: () => void;
  onControl: (action: BrowserControlAction) => void;
  onError: (message: string) => void;
  onFind: () => void;
  onPrint: () => void;
  onScreenshot: () => void;
  onZoom: (delta: number) => void;
  onZoomReset: () => void;
  deviceOpen: boolean;
  onDevice: () => void;
  onDownloads: () => void;
  onHistory: () => void;
  onSettings: (target: "browser" | "site") => void;
  onCopy: () => void;
  onExternal: () => void;
  clearDataControl: ReactNode;
  pageControls?: ReactNode;
  downloadsControl?: ReactNode;
  onMenuOpenChange?: (open: boolean) => void;
}

export function BrowserNavigationControls({
  navigation,
  page,
  address,
  addressField,
  onAddressChange,
  onNavigate,
  onControl,
  onError,
  onFind,
  onPrint,
  onScreenshot,
  onZoom,
  onZoomReset,
  deviceOpen,
  onDevice,
  onDownloads,
  onHistory,
  onSettings,
  onCopy,
  onExternal,
  clearDataControl,
  pageControls,
  downloadsControl,
  onMenuOpenChange,
}: Props) {
  const [menuOpen, setMenuOpen] = useState(false);
  const [editing, setEditing] = useState(false);
  const addressText = useAddressText(editing, page, address);
  const overflow = useToolbarOverflow(menuOpen, pageControls);
  useLayoutEffect(() => {
    if (editing) addressField.current?.select();
  }, [editing, addressField]);
  const menuTrigger = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  useNativePageLayer(menu, { open: menuOpen });
  useDismissOnOutsidePointer(menuOpen, [menu, menuTrigger], setMenuOpen);
  useEffect(() => onMenuOpenChange?.(menuOpen), [menuOpen, onMenuOpenChange]);
  useEffect(() => {
    if (menuOpen)
      menu.current
        ?.querySelector<HTMLButtonElement>('[role="menuitem"]:not(:disabled)')
        ?.focus();
  }, [menuOpen]);
  const run = (action?: () => void) => {
    setMenuOpen(false);
    menuTrigger.current?.focus();
    action?.();
  };
  const onMenuClickCapture = (event: MouseEvent<HTMLDivElement>) => {
    if (!(event.target instanceof Element)) return;
    if (!event.target.closest(".browser-clear-data--menu button")) return;
    run();
  };
  const onMenuKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (menuOpen) browserMenuKeyDown(event, menu.current, run);
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
      <BrowserAnnotateControl
        navigation={navigation}
        onControl={onControl}
        onError={onError}
      />
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
          {page?.transport === "secure" && <Lock size={14} />}
        </span>
        <input
          ref={addressField}
          aria-label="Address or search"
          placeholder="Search or enter a URL"
          type="text"
          maxLength={2048}
          value={addressText}
          onFocus={() => setEditing(true)}
          onBlur={() => setEditing(false)}
          onChange={(event) => onAddressChange(event.target.value)}
          onKeyDown={(event) => {
            if (event.key !== "Escape") return;
            event.preventDefault();
            event.stopPropagation();
            onAddressChange(navigation?.url ?? "");
            event.currentTarget.blur();
          }}
        />
      </form>
      <div className="browser-surface__right-group" onKeyDown={onMenuKeyDown}>
        {overflow.toolbarActions}
        {downloadsControl}
        <button
          ref={menuTrigger}
          type="button"
          aria-label="More options"
          aria-expanded={menuOpen}
          onClick={() => setMenuOpen((value) => !value)}
        >
          <Ellipsis size={18} aria-hidden="true" />
        </button>
        {/* Keep the clear-data confirmation mounted after its menu closes. */}
        <div
          hidden={!menuOpen}
          ref={menu}
          className="browser-surface__menu"
          role="menu"
          aria-label="Browser menu"
          onClickCapture={onMenuClickCapture}
        >
          {overflow.menuActions}
          <div className="browser-surface__menu-group">
            <button
              role="menuitem"
              type="button"
              disabled={!page}
              onClick={() => run(onFind)}
            >
              Find in page
            </button>
            <button
              role="menuitem"
              type="button"
              disabled={!page || Boolean(navigation?.printUnavailableReason)}
              title={
                page
                  ? (navigation?.printUnavailableReason ?? undefined)
                  : "Open a page to print"
              }
              onClick={() => run(onPrint)}
            >
              Print
            </button>
            <button
              role="menuitem"
              type="button"
              disabled={!page}
              onClick={() => run(onScreenshot)}
            >
              Take a screenshot
            </button>
            <div className="browser-surface__menu-zoom">
              <span>Zoom</span>
              <button
                type="button"
                role="menuitem"
                aria-label="Zoom out"
                disabled={!page || (navigation?.zoomPercent ?? 100) <= 50}
                onClick={() => onZoom(-25)}
              >
                −
              </button>
              <span>{navigation?.zoomPercent ?? 100}%</span>
              <button
                type="button"
                role="menuitem"
                aria-label="Zoom in"
                disabled={!page || (navigation?.zoomPercent ?? 100) >= 300}
                onClick={() => onZoom(25)}
              >
                +
              </button>
              <button
                type="button"
                role="menuitem"
                aria-label="Reset zoom"
                disabled={!page}
                onClick={onZoomReset}
              >
                ↺
              </button>
            </div>
            <button
              role="menuitem"
              type="button"
              disabled={
                !deviceOpen &&
                (!page || Boolean(navigation?.deviceUnavailableReason))
              }
              title={
                page
                  ? (navigation?.deviceUnavailableReason ?? undefined)
                  : "Open a page to emulate a device"
              }
              onClick={() => run(onDevice)}
            >
              {deviceOpen ? "Hide device toolbar" : "Show device toolbar"}
            </button>
          </div>
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
            <BrowserSettingsMenuItems run={run} onSettings={onSettings} />
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
      </div>
    </>
  );
}

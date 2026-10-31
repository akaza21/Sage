import { useContext, useEffect, useRef, useState } from "react";
import { LazyModuleBoundary } from "../../ui/LazyModuleBoundary";
import { ClosedBrowserSurface } from "./ClosedBrowserSurface";
import { AgentToolsContext } from "../../workbench/AgentToolsProvider";
import { useWorkbenchStore } from "../../app/store";
import { useBrowserAgentPresence } from "../../app/browserAgentActivityStore";
import type { WorkbenchTab } from "../../domain/workbench";
import {
  browserControl,
  browserCaptureScreenshot,
  browserFind,
  browserHistory,
  browserNavigate,
  browserOpenExternal,
  browserPrint,
  browserStatus,
  browserSuggestedSites,
  browserZoom,
  type BrowserControlAction,
  type BrowserNavigation,
} from "../../ports/nativeBrowserPort";
import { isTauriRuntime } from "../../ports/nativeInvoke";
import {
  browserPageIdentity,
  resolveBrowserAddress,
  suggestedSitesWithTitles,
} from "./browserAddress";
import { BrowserToolbarPageActions } from "./BrowserAttachmentControls";
import { BrowserDownloadsControl } from "./BrowserDownloadsControl";
import { copyPngToClipboard } from "./browserClipboard";
import { BrowserDrawers } from "./BrowserSitePermissions";
import { useAgentSettingsState } from "../settings/agentSettingsState";
import { BrowserClearData } from "./BrowserClearData";
import {
  BrowserFindControls,
  BrowserNavigationControls,
} from "./BrowserNavigationControls";
import {
  performBrowserShortcut,
  useBrowserShortcuts,
} from "./browserShortcuts";
import { useBrowserPlacement } from "./useBrowserPlacement";
import { useBrowserFind } from "./useBrowserFind";
import "./browser.css";
import { loadBrowserNewTab } from "./browserNewTabLoader";

function projectBrowserTabStatus(tabId: string, status: BrowserNavigation) {
  const store = useWorkbenchStore.getState();
  const { favicon, url, title, revision, loading, error } = status;
  store.updateBrowserTabFavicon(tabId, favicon, revision);
  if (!loading && !error && url)
    store.updateBrowserTabTitle(tabId, url, title, revision);
}

export function BrowserSurface({ tab }: { tab: WorkbenchTab }) {
  if (tab.status === "closed") return <ClosedBrowserSurface tabId={tab.id} />;
  return <LiveBrowserSurface tab={tab} />;
}
function LiveBrowserSurface({ tab }: { tab: WorkbenchTab }) {
  const tools = useContext(AgentToolsContext);
  const projectId = useWorkbenchStore((state) => state.project?.id ?? null);
  const tabs = useWorkbenchStore((state) => state.tabs);
  const chats = tabs
    .filter((candidate) => candidate.kind === "chat")
    .map((candidate) => ({ id: candidate.id, title: candidate.title }));
  const frame = useRef<HTMLDivElement>(null);
  const surface = useRef<HTMLElement>(null);
  const addressField = useRef<HTMLInputElement>(null);
  const { findField, findOpen, openFind, closeFind } = useBrowserFind(
    surface,
    addressField,
    () => {
      void pageAction(() => browserFind(tab.id, "stop"));
    },
  );
  const screenshotEpoch = useRef(0);
  const [navigation, setNavigation] = useState<BrowserNavigation | null>(null);
  const latestNavigationRevision = useRef<bigint>(-1n);
  const [suggestionResult, setSuggestionResult] = useState<{
    scope: string;
    sites: ReturnType<typeof suggestedSitesWithTitles>;
  } | null>(null);
  const [address, setAddress] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [screenshotStatus, setScreenshotStatus] = useState<string | null>(null);
  const [downloadsOpen, setDownloadsOpen] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [historyEntries, setHistoryEntries] = useState<string[]>([]);
  const [findText, setFindText] = useState("");
  const [zoomPending, setZoomPending] = useState(false);
  const [attachmentEpoch, setAttachmentEpoch] = useState(0);
  const agentPresence = useBrowserAgentPresence(projectId, tab.id);
  const openBrowser = useWorkbenchStore((state) => state.openBrowser);
  const closeTab = useWorkbenchStore((state) => state.closeTab);
  const page =
    navigation && !navigation.loading
      ? browserPageIdentity(navigation.url)
      : null;
  const newTab =
    navigation?.loading === false && navigation.url === "" && !navigation.error;
  const suggestionScope = `${projectId ?? ""}\0${tab.id}\0${navigation?.revision ?? ""}`;
  const suggestions =
    suggestionResult?.scope === suggestionScope ? suggestionResult.sites : [];
  useEffect(() => {
    if (!newTab) return;
    let active = true;
    void browserSuggestedSites()
      .then(async (sites) => {
        const openTabs = useWorkbenchStore
          .getState()
          .tabs.filter(
            (item) => item.kind === "browser" && item.status !== "closed",
          )
          .slice(0, 8);
        const statuses = await Promise.allSettled(
          openTabs.map((item) => browserStatus(item.id)),
        );
        if (active)
          setSuggestionResult({
            scope: suggestionScope,
            sites: suggestedSitesWithTitles(sites, statuses),
          });
      })
      .catch(() => undefined);
    return () => {
      active = false;
    };
  }, [newTab, suggestionScope]);
  useEffect(() => {
    if (!isTauriRuntime()) return;
    latestNavigationRevision.current = -1n;
    let mounted = true;
    let unlisten: (() => void) | undefined;
    void import("@tauri-apps/api/event")
      .then(async ({ listen }) => {
        const dispose = await listen<BrowserNavigation>(
          "browser:navigation",
          (event) => {
            if (!mounted || event.payload.tabId !== tab.id) return;
            if (!/^(0|[1-9]\d{0,19})$/.test(event.payload.revision)) return;
            const revision = BigInt(event.payload.revision);
            if (revision <= latestNavigationRevision.current) return;
            latestNavigationRevision.current = revision;
            screenshotEpoch.current += 1;
            setScreenshotStatus(null);
            setNavigation(event.payload);
            setError(event.payload.error ?? null);
            if (document.activeElement !== addressField.current) {
              setAddress(event.payload.url);
            }
            projectBrowserTabStatus(tab.id, event.payload);
          },
        );
        if (!mounted) {
          dispose();
          return;
        }
        unlisten = dispose;
        const status = await browserStatus(tab.id);
        if (mounted) {
          if (!/^(0|[1-9]\d{0,19})$/.test(status.revision)) return;
          const revision = BigInt(status.revision);
          if (revision < latestNavigationRevision.current) return;
          latestNavigationRevision.current = revision;
          setNavigation(status);
          setAddress(status.url);
          setError(status.error ?? null);
          projectBrowserTabStatus(tab.id, status);
        }
      })
      .catch((cause) => {
        if (mounted) setError(String(cause).slice(0, 512));
      });
    return () => {
      mounted = false;
      screenshotEpoch.current += 1;
      unlisten?.();
    };
  }, [tab.id]);
  const covered = downloadsOpen || menuOpen;
  useBrowserPlacement(tab.id, frame, surface, setError, covered);
  async function navigate() {
    try {
      setError(null);
      setScreenshotStatus(null);
      screenshotEpoch.current += 1;
      const url = resolveBrowserAddress(address);
      await browserNavigate(tab.id, url);
      setNavigation((current) =>
        current ? { ...current, loading: true } : current,
      );
      addressField.current?.blur();
    } catch (cause) {
      setError(String(cause).slice(0, 512));
    }
  }
  async function control(action: BrowserControlAction) {
    try {
      setError(null);
      await browserControl(tab.id, action);
      if (action === "stop") {
        setNavigation((current) =>
          current ? { ...current, loading: false } : current,
        );
      }
    } catch (cause) {
      setError(String(cause).slice(0, 512));
    }
  }
  async function pageAction(action: () => Promise<void>) {
    try {
      setError(null);
      await action();
    } catch (cause) {
      setError(String(cause).slice(0, 512));
    }
  }
  async function copyScreenshot() {
    const revision = navigation?.revision;
    if (!revision) return;
    const epoch = screenshotEpoch.current;
    setError(null);
    setScreenshotStatus("Capturing screenshot…");
    try {
      const screenshot = await browserCaptureScreenshot(tab.id, revision);
      if (epoch !== screenshotEpoch.current) return;
      await copyPngToClipboard(screenshot.pngBase64);
      if (epoch !== screenshotEpoch.current) return;
      setScreenshotStatus("Screenshot copied to clipboard");
    } catch (cause) {
      if (epoch !== screenshotEpoch.current) return;
      setScreenshotStatus(null);
      setError(
        `Screenshot could not be copied: ${String(cause).slice(0, 384)}`,
      );
    }
  }

  useEffect(() => {
    if (screenshotStatus !== "Screenshot copied to clipboard") return;
    const timeout = setTimeout(() => setScreenshotStatus(null), 5_000);
    return () => clearTimeout(timeout);
  }, [screenshotStatus]);
  function changeZoom(delta: number) {
    const percent = Math.max(
      50,
      Math.min(300, (navigation?.zoomPercent ?? 100) + delta),
    );
    setZoom(percent);
  }

  function setZoom(percent: number) {
    if (zoomPending) return;
    setZoomPending(true);
    void pageAction(async () => {
      await browserZoom(tab.id, percent);
      setNavigation((current) =>
        current ? { ...current, zoomPercent: percent } : current,
      );
    }).finally(() => setZoomPending(false));
  }

  function openHistory() {
    setHistoryOpen(true);
    void browserHistory(tab.id)
      .then(setHistoryEntries)
      .catch((cause) => setError(String(cause).slice(0, 512)));
  }

  useBrowserShortcuts(surface, tab.id, Boolean(navigation?.loading), (action) =>
    performBrowserShortcut(action, {
      address: addressField.current,
      find: () => findField.current,
      control: (command) => void control(command),
      findOpen: openFind,
      zoom: changeZoom,
      zoomPercent: navigation?.zoomPercent ?? 100,
      newTab: () => void openBrowser(),
      closeTab: () => closeTab(tab.id),
    }),
  );

  return (
    <section
      ref={surface}
      aria-label="Browser page"
      className="browser-surface"
    >
      <div className="browser-surface__toolbar">
        <BrowserNavigationControls
          navigation={navigation}
          page={page}
          address={address}
          addressField={addressField}
          onAddressChange={setAddress}
          onNavigate={() => void navigate()}
          onControl={(action) => void control(action)}
          onError={setError}
          onFind={openFind}
          onPrint={() => void pageAction(() => browserPrint(tab.id))}
          onScreenshot={() => void copyScreenshot()}
          onZoom={changeZoom}
          onZoomReset={() => setZoom(100)}
          onDownloads={() => setDownloadsOpen(true)}
          onHistory={openHistory}
          onSettings={(target) =>
            useAgentSettingsState
              .getState()
              .open("Browser", tab.id, target === "site")
          }
          onCopy={() =>
            void pageAction(() => navigator.clipboard.writeText(page!.url))
          }
          onExternal={() => void pageAction(() => browserOpenExternal(tab.id))}
          clearDataControl={<BrowserClearData menuItem />}
          onMenuOpenChange={setMenuOpen}
          downloadsControl={
            <BrowserDownloadsControl
              tabId={tab.id}
              open={downloadsOpen}
              onOpenChange={setDownloadsOpen}
            />
          }
          pageControls={
            <BrowserToolbarPageActions
              projectId={projectId}
              tabId={tab.id}
              ready={page !== null}
              chats={chats}
              pageRevision={String(navigation?.revision ?? "")}
              operation={agentPresence}
              attachmentEpoch={attachmentEpoch}
              onStopped={() => setAttachmentEpoch((current) => current + 1)}
              onError={setError}
            />
          }
        />
      </div>
      {findOpen ? (
        <BrowserFindControls
          findField={findField}
          findText={findText}
          onFindTextChange={setFindText}
          onFind={(action) =>
            void pageAction(() =>
              browserFind(
                tab.id,
                action,
                action === "start" ? findText : undefined,
              ),
            )
          }
          onClose={closeFind}
        />
      ) : null}
      {error ? (
        <div className="browser-surface__error" role="alert">
          {error}
        </div>
      ) : null}
      {screenshotStatus ? (
        <p className="browser-surface__status" role="status">
          {screenshotStatus}
        </p>
      ) : null}
      <BrowserDrawers
        tabId={tab.id}
        historyOpen={historyOpen}
        historyEntries={historyEntries}
        onCloseHistory={() => setHistoryOpen(false)}
        onNavigate={(url) =>
          void pageAction(() => browserNavigate(tab.id, url))
        }
      />
      {newTab ? (
        <LazyModuleBoundary
          load={loadBrowserNewTab}
          label="New tab"
          render={(BrowserNewTab) => (
            <BrowserNewTab
              suggestions={suggestions}
              onNavigate={(url) =>
                void pageAction(() => browserNavigate(tab.id, url))
              }
              onNewTab={() => void openBrowser()}
              onTool={tools.launch}
              chatAvailable={tools.chatAvailable}
              terminalAvailable={tools.terminalAvailable}
            />
          )}
        />
      ) : null}
      <div
        ref={frame}
        className={`browser-surface__frame${newTab ? " browser-surface__frame--new-tab" : ""}`}
      />
    </section>
  );
}

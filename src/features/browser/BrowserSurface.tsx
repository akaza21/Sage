import { lazy, Suspense, useContext, useEffect, useRef, useState } from "react";
import { AgentToolsContext } from "../../workbench/AgentToolsProvider";
import { useWorkbenchStore } from "../../app/store";
import { useBrowserAgentPresence } from "../../app/browserAgentActivityStore";
import type { WorkbenchTab } from "../../domain/workbench";
import {
  browserControl,
  browserFind,
  browserNavigate,
  browserOpenExternal,
  browserPrint,
  browserStatus,
  browserZoom,
  type BrowserControlAction,
  type BrowserNavigation,
} from "../../ports/nativeBrowserPort";
import { isTauriRuntime } from "../../ports/nativeInvoke";
import { browserPageIdentity, resolveBrowserAddress } from "./browserAddress";
import type { BrowserSuggestedSites } from "./browserSuggestions";
import { BrowserToolbarPageActions } from "./BrowserAttachmentControls";
import { subscribeBrowserTab } from "./browserTabEvents";
import { BrowserDownloadsControl } from "./BrowserDownloadsControl";
import { BrowserSitePermissions } from "./BrowserSitePermissions";
import { useAgentSettingsState } from "../settings/agentSettingsState";
import { BrowserClearData } from "./BrowserClearData";
import { BrowserNavigationControls } from "./BrowserNavigationControls";
import {
  performBrowserShortcut,
  useBrowserShortcuts,
} from "./browserShortcuts";
import { useBrowserDeviceTrigger } from "./useBrowserDeviceTrigger";
import { useBrowserPlacement } from "./useBrowserPlacement";
import { useBrowserScreenshot } from "./useBrowserScreenshot";
import { projectBrowserTabStatus } from "./browserTabProjection";
import "./browser.css";

const ClosedBrowserSurface = lazy(() => import("./ClosedBrowserSurface"));
const BrowserFindControls = lazy(() =>
  import("./BrowserFindControls").then((module) => ({
    default: module.BrowserFindControls,
  })),
);
// The device toolbar's state, grips and styles load when it is first shown
// or a tab restores an emulation; the surface keeps only whether it is open.
const BrowserDeviceController = lazy(() =>
  import("./BrowserDeviceController").then((module) => ({
    default: module.BrowserDeviceController,
  })),
);
const BrowserNewTab = lazy(() =>
  import("./BrowserNewTab").then((module) => ({
    default: module.BrowserNewTab,
  })),
);
const BrowserHistoryPage = lazy(() =>
  import("./BrowserHistoryPage").then((module) => ({
    default: module.BrowserHistoryPage,
  })),
);

export function BrowserSurface({ tab }: { tab: WorkbenchTab }) {
  if (tab.status === "closed")
    return (
      <Suspense fallback={null}>
        <ClosedBrowserSurface tabId={tab.id} />
      </Suspense>
    );
  return <LiveBrowserSurface tab={tab} />;
}
function LiveBrowserSurface({ tab }: { tab: WorkbenchTab }) {
  const tools = useContext(AgentToolsContext);
  const projectId = useWorkbenchStore((state) => state.project?.id ?? null);
  const tabs = useWorkbenchStore((state) => state.tabs);
  const chats = tabs.filter((candidate) => candidate.kind === "chat");
  const frame = useRef<HTMLDivElement>(null);
  const viewport = useRef<HTMLDivElement>(null);
  const surface = useRef<HTMLElement>(null);
  const addressField = useRef<HTMLInputElement>(null);
  const findField = useRef<HTMLInputElement>(null);
  const [navigation, setNavigation] = useState<BrowserNavigation | null>(null);
  const [suggestionResult, setSuggestionResult] = useState<{
    scope: string;
    sites: BrowserSuggestedSites;
  } | null>(null);
  const [address, setAddress] = useState("");
  const [error, setError] = useState<string | null>(null);
  const {
    status: screenshotStatus,
    setStatus: setScreenshotStatus,
    epoch: screenshotEpoch,
    copy: copyScreenshot,
  } = useBrowserScreenshot(tab.id, setError);
  const [findOpen, setFindOpen] = useState(false);
  const [downloadsOpen, setDownloadsOpen] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [findText, setFindText] = useState("");
  const [zoomPending, setZoomPending] = useState(false);
  const [attachmentEpoch, setAttachmentEpoch] = useState(0);
  const agentPresence = useBrowserAgentPresence(projectId, tab.id);
  const openBrowser = useWorkbenchStore((state) => state.openBrowser);
  const closeTab = useWorkbenchStore((state) => state.closeTab);
  const ready = navigation?.loading === false;
  const page = ready ? browserPageIdentity(navigation.url) : null;
  const newTab = ready && navigation.url === "" && !navigation.error;
  const newTabVisible = newTab && !historyOpen;
  const suggestionScope = `${projectId ?? ""}\0${tab.id}\0${navigation?.revision ?? ""}`;
  const suggestions =
    suggestionResult?.scope === suggestionScope ? suggestionResult.sites : [];
  if (!newTabVisible && suggestionResult !== null) setSuggestionResult(null);
  useEffect(() => {
    if (!newTabVisible) return;
    let active = true;
    void import("./browserSuggestions")
      .then((module) => (active ? module.browserSuggestions() : []))
      .then((sites) => {
        if (active)
          setSuggestionResult({
            scope: suggestionScope,
            sites,
          });
      })
      .catch(() => undefined);
    return () => {
      active = false;
    };
  }, [newTabVisible, suggestionScope]);
  useEffect(() => {
    if (!isTauriRuntime()) return;
    let latestNavigationRevision = -1n;
    const acceptNavigation = (next: BrowserNavigation, bootstrap = false) => {
      if (!/^(0|[1-9]\d{0,19})$/.test(next.revision)) return;
      const revision = BigInt(next.revision);
      if (
        bootstrap
          ? revision < latestNavigationRevision
          : revision <= latestNavigationRevision
      )
        return;
      latestNavigationRevision = revision;
      if (!bootstrap) {
        screenshotEpoch.current += 1;
        setScreenshotStatus(null);
      }
      setNavigation(next);
      setError(next.error ?? null);
      if (bootstrap || document.activeElement !== addressField.current)
        setAddress(next.url);
      projectBrowserTabStatus(tab.id, next);
    };
    const stop = subscribeBrowserTab(
      "browser:navigation",
      tab.id,
      acceptNavigation,
      () => browserStatus(tab.id),
      setError,
    );
    return () => {
      screenshotEpoch.current += 1;
      stop();
    };
  }, [tab.id, screenshotEpoch, setScreenshotStatus]);
  useBrowserPlacement(tab.id, viewport, surface, setError, historyOpen);
  const status = navigation?.tabId === tab.id ? navigation : null;
  const device = useBrowserDeviceTrigger(tab.id, status);
  function navigate() {
    return pageAction(async () => {
      setHistoryOpen(false);
      setScreenshotStatus(null);
      screenshotEpoch.current += 1;
      const url = resolveBrowserAddress(address);
      await browserNavigate(tab.id, url);
      setNavigation((current) =>
        current ? { ...current, loading: true } : current,
      );
      addressField.current?.blur();
    });
  }
  function control(action: BrowserControlAction) {
    return pageAction(async () => {
      if (action !== "stop") setHistoryOpen(false);
      await browserControl(tab.id, action);
      if (action === "stop") {
        setNavigation((current) =>
          current ? { ...current, loading: false } : current,
        );
      }
    });
  }
  async function pageAction(action: () => Promise<void>) {
    try {
      setError(null);
      await action();
    } catch (cause) {
      setError(String(cause).slice(0, 512));
    }
  }
  function navigateTo(url: string) {
    setHistoryOpen(false);
    void pageAction(() => browserNavigate(tab.id, url));
  }
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

  function closeFind() {
    setFindOpen(false);
    void pageAction(() => browserFind(tab.id, "stop"));
  }

  const openNewTab = () =>
    void openBrowser().then((message) => {
      if (message) setError(message);
    });
  const stoppable = Boolean(navigation?.loading) && !historyOpen;
  useBrowserShortcuts(surface, tab.id, stoppable, (action) =>
    performBrowserShortcut(action, {
      address: addressField.current,
      find: () => findField.current,
      control,
      findOpen: () => setFindOpen(true),
      history: () => setHistoryOpen(true),
      zoom: changeZoom,
      zoomPercent: navigation?.zoomPercent ?? 100,
      newTab: openNewTab,
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
          onNavigate={navigate}
          onControl={control}
          onError={setError}
          onFind={() => setFindOpen(true)}
          onPrint={() => void pageAction(() => browserPrint(tab.id))}
          onScreenshot={() => void copyScreenshot(navigation?.revision)}
          onZoom={changeZoom}
          onZoomReset={() => setZoom(100)}
          deviceOpen={device.open}
          onDevice={() => device.setOpen(!device.open)}
          onDownloads={() => setDownloadsOpen(true)}
          onHistory={() => setHistoryOpen(true)}
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
          downloadsControl={
            <BrowserDownloadsControl
              tabId={tab.id}
              open={downloadsOpen}
              onOpenChange={setDownloadsOpen}
            />
          }
          pageControls={(visible) => (
            <BrowserToolbarPageActions
              visible={visible}
              projectId={projectId}
              tabId={tab.id}
              ready={Boolean(page)}
              chats={chats}
              pageRevision={navigation?.revision ?? ""}
              operation={agentPresence}
              attachmentEpoch={attachmentEpoch}
              onStopped={() => setAttachmentEpoch((current) => current + 1)}
              onError={setError}
            />
          )}
        />
      </div>
      {device.loaded ? (
        <Suspense
          fallback={
            device.open ? (
              <div className="browser-surface__device-slot" />
            ) : null
          }
        >
          <BrowserDeviceController
            tabId={tab.id}
            open={device.open}
            restored={device.restored}
            frame={frame}
            onOpenChange={device.setOpen}
            onError={setError}
          />
        </Suspense>
      ) : null}
      {findOpen ? (
        <Suspense fallback={null}>
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
        </Suspense>
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
      <BrowserSitePermissions tabId={tab.id} />
      {historyOpen ? (
        <Suspense fallback={<p role="status">Loading history…</p>}>
          <BrowserHistoryPage
            onNavigate={navigateTo}
            onClose={() => {
              setHistoryOpen(false);
              requestAnimationFrame(() => addressField.current?.focus());
            }}
          />
        </Suspense>
      ) : newTab ? (
        <Suspense fallback={<p role="status">Loading new tab…</p>}>
          <BrowserNewTab
            suggestions={suggestions}
            onNavigate={navigateTo}
            onNewTab={openNewTab}
            onTool={tools.launch}
            chatAvailable={tools.chatAvailable}
            terminalAvailable={tools.terminalAvailable}
          />
        </Suspense>
      ) : null}
      <div
        ref={frame}
        className={`browser-surface__frame${newTab || historyOpen ? " browser-surface__frame--new-tab" : ""}`}
      >
        <div ref={viewport} className="browser-surface__viewport" />
      </div>
    </section>
  );
}

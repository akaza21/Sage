import {
  Activity,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ComponentType,
  type KeyboardEvent,
} from "react";
import { LazyModuleBoundary } from "../../ui/LazyModuleBoundary";
import { ModalDialog } from "../../ui/ModalDialog";
import {
  hasNavigationModifier,
  listIndexForKey,
} from "../../ui/listNavigation";
import { useDocumentVisible } from "../../ui/useDocumentVisible";
import { ChevronRight, X } from "../../ui/leanIcons";
import { matchingSettingsEntryIds } from "../../domain/settingsNavigation";
import { loadSettingsPasswordsPage } from "../../workbench/settings/settingsPageLoaders";
import "./agent-settings.css";
import { AgentAppearanceSettings } from "./AgentAppearanceSettings";
import { AgentChatSettings } from "./AgentChatSettings";
import type { HarnessNoticePresentation } from "../chat/harnessNoticePresentation";
import { SettingsHarnessNotices } from "./SettingsHarnessNotices";
import { HarnessDiagnostics } from "./HarnessDiagnostics";
import {
  returnFocusToAgentSettingsOpener,
  useAgentSettingsState,
  type AgentSettingsPage,
} from "./agentSettingsState";

const loadBrowserSettings = () =>
  import("./AgentBrowserSettings").then(({ AgentBrowserSettings }) => ({
    default: AgentBrowserSettings,
  }));

// The Harnesses page loads on demand, and Settings keeps the current page
// on screen until it is ready rather than flashing a loading line. It is
// fetched ahead when Settings goes idle, or when its section is pointed at.
type HarnessSettingsView = ComponentType<{
  presentation: HarnessNoticePresentation;
}>;
let HarnessSettings: HarnessSettingsView | null = null;
let harnessRequest: Promise<HarnessSettingsView> | null = null;
const loadHarnessSettings = () =>
  (harnessRequest ??= import("./AgentHarnessSettings").then(
    ({ AgentHarnessSettings }) => (HarnessSettings = AgentHarnessSettings),
    (error: unknown) => {
      harnessRequest = null;
      throw error;
    },
  ));
const prewarmHarnessSettings = () =>
  void loadHarnessSettings().catch(() => undefined);
const loadHarnessModule = () =>
  loadHarnessSettings().then((Surface) => ({ default: Surface }));

const loadExecution = () => import("./AgentSettingsExecution");
// The dialog has no per-setting search, so the whole Passwords page shows.
const passwordEntries = matchingSettingsEntryIds("", "passwords");
// One chunk serves both of the last two pages.
const loadArchivedSettings = () => import("./AgentArchivedSettings");

export default AgentSettingsSurface;

/** Agent settings only: editor and keybinding settings live in the Editor. */
const pages: Readonly<Record<AgentSettingsPage, string>> = {
  Appearance:
    "How the Agent window looks. Code font, tab size and keybindings are Editor settings.",
  Harnesses: "Installed harnesses, sign-in and versions.",
  Chats: "Chat defaults and history.",
  Browser: "The in-app browser used for previews.",
  Passwords: "Saved sign-ins for the in-app browser.",
  "Archived chats": "Chats you archived.",
  Data: "Storage and local data.",
};
const pageNames = Object.keys(pages) as AgentSettingsPage[];

const textEntry =
  "input:not([type]), input[type=text], input[type=email], input[type=password], input[type=url], input[type=tel], input[type=number], input[type=date], input[type=time], input[type=datetime-local], input[type=month], input[type=week], textarea, [contenteditable=true]";

/** Escape belongs to a field being typed in, not to the dialog. */
function keepsEscape(target: Element | null) {
  return Boolean(
    target?.closest(textEntry) ||
    (target instanceof HTMLInputElement &&
      target.type === "search" &&
      target.value),
  );
}

function moveSectionFocus(event: KeyboardEvent<HTMLElement>) {
  if (hasNavigationModifier(event)) return;
  const sections = [
    ...event.currentTarget.querySelectorAll<HTMLButtonElement>(
      ".agent-settings__section",
    ),
  ];
  const index = sections.indexOf(event.target as HTMLButtonElement);
  if (index < 0) return;
  const next = listIndexForKey(
    event.key,
    index,
    sections.map(() => false),
    { orientation: "vertical", wrap: true },
  );
  if (next === null) return;
  event.preventDefault();
  sections[next].focus();
}

export function AgentSettingsSurface({
  chatRuntimeOwned = false,
  onOpenEditorSettings,
}: {
  chatRuntimeOwned?: boolean;
  /** Leaves Agent mode for the Editor's Settings tab. */
  onOpenEditorSettings?: () => void;
}) {
  const documentVisible = useDocumentVisible();
  const page = useAgentSettingsState((state) => state.page) ?? "Appearance";
  const open = useAgentSettingsState((state) => state.open);
  const close = useAgentSettingsState((state) => state.close);
  const surfaceRef = useRef<HTMLDivElement | null>(null);
  const pageRef = useRef<HTMLDivElement | null>(null);
  const [initialSection] = useState(page);
  const initialFocusRef = useRef<HTMLButtonElement | null>(null);
  const [, setHarnessReady] = useState(Boolean(HarnessSettings));
  const [harnessFailed, setHarnessFailed] = useState(false);
  const [shownPage, setShownPage] = useState(page);
  const deferredFrom = useRef<Element | null>(null);
  if (
    page !== shownPage &&
    (page !== "Harnesses" || HarnessSettings || harnessFailed)
  )
    setShownPage(page);
  useEffect(() => {
    if (page !== "Harnesses" || HarnessSettings) return;
    deferredFrom.current = document.activeElement;
    let active = true;
    loadHarnessSettings().then(
      () => active && setHarnessReady(true),
      () => active && setHarnessFailed(true),
    );
    return () => {
      active = false;
    };
  }, [page]);
  // A layout effect, so focus moves in the same commit that shows the page
  // and nothing can see the page on screen before it holds focus.
  useLayoutEffect(() => {
    if (shownPage !== "Harnesses" || !deferredFrom.current) return;
    const from = deferredFrom.current;
    deferredFrom.current = null;
    // Move into the page only if focus never left the section button that
    // asked for it, so moving on while it loads is kept.
    if (document.activeElement !== from) return;
    pageRef.current
      ?.querySelector<HTMLElement>("button, input, select, textarea, a[href]")
      ?.focus();
  }, [shownPage]);
  useEffect(() => {
    const idle = window.requestIdleCallback ?? window.setTimeout;
    const cancel = window.cancelIdleCallback ?? window.clearTimeout;
    const handle = idle(prewarmHarnessSettings);
    return () => cancel(handle);
  }, []);
  // Closing hands focus back to whatever opened the dialog.
  useEffect(() => {
    const surface = surfaceRef.current;
    return () => returnFocusToAgentSettingsOpener(surface);
  }, []);
  return (
    <ModalDialog
      className="agent-settings"
      labelledBy="agent-settings-label"
      initialFocusRef={initialFocusRef}
      // The frame fills the dialog, so only the backdrop hits it directly.
      onPointerDown={(event) => {
        if (event.target === event.currentTarget) close();
      }}
      onDismiss={() => {
        if (!keepsEscape(document.activeElement)) close();
      }}
    >
      <div
        ref={surfaceRef}
        className="agent-settings__frame"
        onKeyDown={(event) => {
          // Left to the field, so the dialog's Escape does not take it.
          if (event.key === "Escape" && keepsEscape(event.target as Element))
            event.stopPropagation();
        }}
      >
        <span id="agent-settings-label" hidden>
          Agent settings
        </span>
        <nav
          className="agent-settings__nav"
          aria-label="Agent settings sections"
          onKeyDown={moveSectionFocus}
        >
          <h2 className="agent-settings__title">Settings</h2>
          {pageNames.map((item) => (
            <button
              key={item}
              ref={item === initialSection ? initialFocusRef : undefined}
              type="button"
              className="agent-settings__section"
              aria-current={page === item ? "page" : undefined}
              onClick={() => open(item)}
              {...(item === "Harnesses"
                ? {
                    onPointerEnter: prewarmHarnessSettings,
                    onFocus: prewarmHarnessSettings,
                  }
                : {})}
            >
              {item}
            </button>
          ))}
          {onOpenEditorSettings ? (
            <button
              type="button"
              className="agent-settings__editor-link"
              onClick={onOpenEditorSettings}
            >
              Editor settings
              <ChevronRight size={14} aria-hidden="true" />
            </button>
          ) : null}
        </nav>
        <div className="agent-settings__content">
          {chatRuntimeOwned ? null : (
            <LazyModuleBoundary
              label="Chat execution"
              load={loadExecution}
              fallback={null}
              render={(Execution) => <Execution />}
            />
          )}
          {/* Like the Editor workbench, pages sleep while the window is
              minimized, so they clear what they hold on suspend. */}
          <Activity mode={documentVisible ? "visible" : "hidden"}>
            <div className="agent-settings__page" key={shownPage} ref={pageRef}>
              <header className="agent-settings__header">
                <h2>{shownPage}</h2>
                <p>{pages[shownPage]}</p>
              </header>
              {shownPage === "Appearance" ? <AgentAppearanceSettings /> : null}
              {shownPage === "Harnesses" ? (
                HarnessSettings ? (
                  <HarnessSettings presentation={SettingsHarnessNotices} />
                ) : (
                  <LazyModuleBoundary
                    label="Harness settings"
                    load={loadHarnessModule}
                    render={(Surface) => (
                      <Surface presentation={SettingsHarnessNotices} />
                    )}
                  />
                )
              ) : null}
              {shownPage === "Chats" ? (
                <AgentChatSettings presentation={SettingsHarnessNotices} />
              ) : null}
              {shownPage === "Harnesses" ? <HarnessDiagnostics /> : null}
              {shownPage === "Browser" ? (
                <LazyModuleBoundary
                  label="Browser settings"
                  load={loadBrowserSettings}
                  render={(AgentBrowserSettings) => <AgentBrowserSettings />}
                />
              ) : null}
              {shownPage === "Passwords" ? (
                <LazyModuleBoundary
                  label="Saved passwords"
                  load={loadSettingsPasswordsPage}
                  render={(SettingsPasswordsPage) => (
                    <SettingsPasswordsPage visible={passwordEntries} />
                  )}
                />
              ) : null}
              {shownPage === "Archived chats" || shownPage === "Data" ? (
                <LazyModuleBoundary
                  label={shownPage}
                  load={loadArchivedSettings}
                  render={(DataPage) => <DataPage page={shownPage} />}
                />
              ) : null}
            </div>
          </Activity>
        </div>
        <button
          type="button"
          className="icon-button agent-settings__close"
          aria-label="Close settings"
          onClick={close}
        >
          <X size={16} aria-hidden="true" />
        </button>
      </div>
    </ModalDialog>
  );
}

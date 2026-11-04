import {
  DISCONNECTED_TERMINAL,
  type TerminalDockProjection,
} from "./terminalDockProjection";
import { useTerminalDockStore } from "../app/terminalDockStore";
import {
  formatKeymapChords,
  keymapBinding,
  matchesPlatformChord,
} from "../domain/keymapRegistry";
import { usesMetaModifier } from "../ui/shortcutLabel";

/** One command owner for buttons, launchers and keys, including focus return. */
export function connectTerminalDock(
  blocked: () => boolean,
  onChange: (snapshot: TerminalDockProjection) => void,
) {
  let returnFocus: WeakRef<HTMLElement> | null = null;
  let focusObserver: MutationObserver | null = null;
  const clearReturnFocus = () => {
    returnFocus = null;
    focusObserver?.disconnect();
    focusObserver = null;
  };
  const setOpen = (open: boolean) => {
    const state = useTerminalDockStore.getState();
    if (!state.ready || state.visible === open) return;
    if (open) {
      clearReturnFocus();
      if (
        document.activeElement instanceof HTMLElement &&
        document.activeElement !== document.body &&
        !document.activeElement.closest(".terminal-dock")
      ) {
        returnFocus = new WeakRef(document.activeElement);
        focusObserver = new MutationObserver(() => {
          if (!returnFocus?.deref()?.isConnected) clearReturnFocus();
        });
        focusObserver.observe(document.body, {
          childList: true,
          subtree: true,
        });
      }
    }
    const target = !open ? returnFocus?.deref() : null;
    if (!open) clearReturnFocus();
    state.setVisible(open);
    if (!open) {
      const valid =
        target?.isConnected &&
        target !== document.body &&
        !target.closest(".terminal-dock");
      if (valid) target.focus();
      if (!valid || document.activeElement !== target)
        document
          .querySelector<HTMLButtonElement>(".status-bar__terminal")
          ?.focus();
    }
  };
  const toggle = () => setOpen(!useTerminalDockStore.getState().visible);
  const keyDown = (event: KeyboardEvent) => {
    if (
      !useTerminalDockStore.getState().ready ||
      blocked() ||
      event.defaultPrevented ||
      event.isComposing ||
      document.querySelector("dialog[open]")
    )
      return;
    const chord = matchesPlatformChord(
      keymapBinding("agent.terminal"),
      event,
      usesMetaModifier(),
    );
    const escape =
      event.key === "Escape" &&
      !event.ctrlKey &&
      !event.metaKey &&
      !event.altKey &&
      !event.shiftKey &&
      event.target instanceof Element &&
      event.target.closest(".terminal-dock__header");
    if ((!chord && !escape) || event.repeat) return;
    event.preventDefault();
    event.stopPropagation();
    if (chord) toggle();
    else setOpen(false);
  };
  // Capture the toggle before xterm translates it into PTY input. Escape only
  // dismisses dock chrome; terminal applications receive their own Escape.
  window.addEventListener("keydown", keyDown, true);
  const commands = {
    controls: <TerminalControls onToggle={toggle} />,
    setOpen,
    open: () => setOpen(true),
  };
  const publish = () => {
    const { visible } = useTerminalDockStore.getState();
    onChange({ visible, ...commands });
  };
  const unsubscribe = useTerminalDockStore.subscribe((state, previous) => {
    if (!state.visible || !returnFocus?.deref()?.isConnected)
      clearReturnFocus();
    if (state.visible !== previous.visible) publish();
  });
  publish();
  void useTerminalDockStore.getState().restore();
  return () => {
    clearReturnFocus();
    window.removeEventListener("keydown", keyDown, true);
    unsubscribe();
    onChange(DISCONNECTED_TERMINAL);
  };
}

function TerminalControls({ onToggle }: { onToggle: () => void }) {
  const { visible, ready, error } = useTerminalDockStore();
  return (
    <>
      {error ? <span role="status">{error}</span> : null}
      {ready ? (
        <button
          className="status-bar__button status-bar__terminal"
          type="button"
          aria-label="Toggle terminal"
          aria-pressed={visible}
          title={`${visible ? "Hide" : "Show"} terminal (${formatKeymapChords(keymapBinding("agent.terminal"), usesMetaModifier()).join(" / ")})`}
          onPointerDown={(event) => event.preventDefault()}
          onClick={onToggle}
        >
          <span aria-hidden="true">&gt;_</span> Terminal
        </button>
      ) : null}
    </>
  );
}

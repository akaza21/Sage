import type { EditorView } from "@codemirror/view";

/**
 * Disabling the editor while it saves moves the browser's focus to the page.
 * This gives focus back when the editor is enabled again, unless the user
 * moved it or clicked elsewhere meanwhile (WKWebView does not focus a clicked
 * button, so a click can leave focus on the page).
 */
export function editorSaveRefocus(view: EditorView) {
  let pending = false;
  const page = () => view.dom.ownerDocument;
  const cancel = () => {
    pending = false;
    page().removeEventListener("pointerdown", clickedAway, true);
  };
  const clickedAway = (event: Event) => {
    if (!view.dom.contains(event.target as Node)) cancel();
  };
  return {
    /** Call before the editor is reconfigured as disabled. */
    disabled() {
      if (!view.hasFocus) return;
      pending = true;
      page().addEventListener("pointerdown", clickedAway, true);
    },
    enabled() {
      if (!pending) return;
      cancel();
      const active = page().activeElement;
      if (!active || active === page().body) view.focus();
    },
    cancel,
  };
}

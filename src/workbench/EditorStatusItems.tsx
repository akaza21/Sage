import { useEditorCaretStore } from "../app/editorCaretStore";
import { useEditorSettingsStore } from "../app/editorSettingsStore";
import { useWorkbenchStore } from "../app/store";
import { caretStatusLabel } from "../features/files/editorCaret";
import { editorLanguageStatusLabel } from "../features/files/editorLanguage";

/** Editor-only subscriptions live with their visible status items. */
export function EditorStatusItems() {
  const tabSize = useEditorSettingsStore(
    (state) => state.settings["editor.tabSize"],
  );
  const caretLabel = caretStatusLabel(
    useEditorCaretStore((state) => state.caret),
  );
  const languageLabel = editorLanguageStatusLabel(
    useWorkbenchStore((state) => {
      const tab = state.tabs.find((item) => item.id === state.activeTabId);
      return tab?.kind === "file"
        ? (tab.resource?.relativePath ?? tab.title)
        : null;
    }),
  );
  return (
    <>
      {caretLabel ? <span title="Caret position">{caretLabel}</span> : null}
      <span title="Indentation">Spaces: {tabSize}</span>
      {languageLabel ? (
        <span title="Editor language">{languageLabel}</span>
      ) : null}
    </>
  );
}

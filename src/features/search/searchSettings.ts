import {
  DEFAULT_EDITOR_SETTINGS,
  type EditorSettings,
  sameEditorSettingValue,
} from "../../domain/editorSettingsCatalogue";

const SEARCH_SETTINGS = ["search.useIgnoreFiles", "files.exclude"] as const;

export function sameSearchSettings(
  next: EditorSettings,
  previous: EditorSettings,
): boolean {
  return SEARCH_SETTINGS.every((key) =>
    sameEditorSettingValue(next[key], previous[key]),
  );
}

/** Sends a field only when it differs from its default, so a default search carries none. */
export function searchSettingFields(
  useIgnoreFiles: boolean,
  filesExclude: EditorSettings["files.exclude"],
) {
  return {
    ...(useIgnoreFiles ? {} : { useIgnoreFiles: false }),
    ...(sameEditorSettingValue(
      filesExclude,
      DEFAULT_EDITOR_SETTINGS["files.exclude"],
    )
      ? {}
      : { filesExclude: [...filesExclude] }),
  };
}

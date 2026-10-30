import {
  DEFAULT_EDITOR_SETTINGS,
  type EditorSettings,
  sameEditorSettingValue,
} from "../../domain/editorSettingsCatalogue";

/** Settings that change what a Search finds; a change retires results. */
const SEARCH_SETTINGS = ["search.useIgnoreFiles", "files.exclude"] as const;

export function sameSearchSettings(
  next: EditorSettings,
  previous: EditorSettings,
): boolean {
  return SEARCH_SETTINGS.every((key) =>
    sameEditorSettingValue(next[key], previous[key]),
  );
}

/**
 * The request fields for those settings, sent only when they differ from the
 * native defaults so a default search carries no extra fields.
 */
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

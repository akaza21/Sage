import type { EditorSettingKey } from "./editorSettingsCatalogue";

/**
 * Kept out of the catalogue, which loads at startup, so these load with the
 * Settings page.
 */

export const EDITOR_SETTING_GROUPS = [
  "Text",
  "Editing",
  "Files",
  "Search",
  "Git",
  "Languages",
] as const;

export type EditorSettingGroup = (typeof EDITOR_SETTING_GROUPS)[number];

export interface EditorSettingCopy {
  group: EditorSettingGroup;
  label: string;
  description: string;
  unit?: string;
  /** Choice settings: display labels as [other platforms, macOS]. */
  choiceLabels?: Readonly<Record<string, readonly [string, string]>>;
}

export const EDITOR_SETTING_COPY: Readonly<
  Record<EditorSettingKey, EditorSettingCopy>
> = {
  "editor.fontSize": {
    group: "Text",
    label: "Font size",
    description: "Controls the size of text in the code editor.",

    unit: "px",
  },
  "editor.lineNumbers": {
    group: "Text",
    label: "Line numbers",
    description: "Shows a numbered gutter beside each line.",
  },
  "editor.wordWrap": {
    group: "Text",
    label: "Word wrap",
    description: "Wraps long lines so they remain inside the Editor window.",
  },
  "editor.renderWhitespace": {
    group: "Text",
    label: "Show whitespace",
    description: "Reveals spaces and tabs when indentation is unclear.",
  },
  "editor.tabSize": {
    group: "Editing",
    label: "Tab size",
    description: "Sets the number of spaces used for indentation.",

    unit: "spaces",
  },
  "editor.matchBrackets": {
    group: "Text",
    label: "Match brackets",
    description:
      "Highlights the bracket that pairs with the one at the cursor.",
  },
  "editor.autoClosingBrackets": {
    group: "Editing",
    label: "Auto-close brackets",
    description:
      "Types the closing bracket or quote when you type the opening one.",
  },
  "editor.folding": {
    group: "Text",
    label: "Code folding",
    description: "Shows fold arrows in the gutter so blocks can be collapsed.",
  },
  "editor.occurrencesHighlight": {
    group: "Text",
    label: "Highlight occurrences",
    description:
      "Marks other copies of the selected text or the word at the cursor.",
  },
  "editor.multiCursorModifier": {
    group: "Editing",
    label: "Multi-cursor modifier",
    description: "The key to hold while clicking to add another cursor.",

    choiceLabels: { alt: ["Alt", "Option"], ctrlCmd: ["Ctrl", "Cmd"] },
  },
  "search.useIgnoreFiles": {
    group: "Search",
    label: "Use ignore files",
    description:
      "Search skips files that ignore rules match: .gitignore, .ignore and Git's exclude files.",
  },
  "explorer.showIgnored": {
    group: "Files",
    label: "Ignored files in Explorer",
    description:
      "Shows files that ignore rules match in a dimmed color, or hides them.",

    choiceLabels: {
      dimmed: ["Dimmed", "Dimmed"],
      hidden: ["Hidden", "Hidden"],
    },
  },
  "files.exclude": {
    group: "Files",
    label: "Exclude",
    description:
      "Glob patterns for files and folders that Explorer and Search leave out, such as **/dist or **/*.log.",
  },
  "git.autoRefresh": {
    group: "Git",
    label: "Auto refresh",
    description:
      "Updates Source Control when files or Git state change outside Sage, such as a commit from a terminal.",
  },
  "git.decorations": {
    group: "Git",
    label: "Decorations",
    description:
      "Shows Git status in Explorer and on tabs: a colored name and a letter for each changed file, and a dot on folders that contain changes.",
  },
  "editor.gitGutter": {
    group: "Git",
    label: "Change markers",
    description:
      "Marks lines you have added, changed or deleted since the last stage or commit next to the line numbers. Click a marker to see what was there and revert it.",
  },
};

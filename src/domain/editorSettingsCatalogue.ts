/**
 * The editor settings catalogue (EDIT-0005 AD-2). One closed registry lists
 * each key with its type, default, range and owning ticket, plus the copy and
 * group the Editor settings page renders it with. It mirrors `EDITOR_SETTINGS`
 * in `src-tauri/src/settings_store/editor_settings.rs`, and a parity test keeps
 * the two equal. Adding a setting is one entry here, one there, and its
 * control: never a schema change.
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

interface EntryCopy {
  key: `editor.${string}`;
  group: EditorSettingGroup;
  ticket: `EP-${number}`;
  label: string;
  description: string;
}

export interface BooleanSettingSpec extends EntryCopy {
  kind: "boolean";
  default: boolean;
}

export interface IntegerSettingSpec extends EntryCopy {
  kind: "integer";
  default: number;
  min: number;
  max: number;
  /** The only allowed values, offered as a list on the settings page. */
  choices: readonly number[];
  /** Shown after the number, such as "px". */
  unit: string;
}

export interface ChoiceSettingSpec extends EntryCopy {
  kind: "choice";
  default: string;
  /** The allowed names, in the order the settings page lists them. */
  choices: readonly string[];
  /** Each name's label elsewhere and on macOS, such as "Alt" and "Option". */
  choiceLabels: Readonly<Record<string, readonly [string, string]>>;
}

export type EditorSettingSpec =
  BooleanSettingSpec | IntegerSettingSpec | ChoiceSettingSpec;

export const EDITOR_SETTINGS = [
  {
    key: "editor.fontSize",
    kind: "integer",
    default: 13,
    min: 12,
    max: 16,
    choices: [12, 13, 14, 16],
    unit: "px",
    group: "Text",
    ticket: "EP-02",
    label: "Font size",
    description: "Controls the size of text in the code editor.",
  },
  {
    key: "editor.lineNumbers",
    kind: "boolean",
    default: true,
    group: "Text",
    ticket: "EP-02",
    label: "Line numbers",
    description: "Shows a numbered gutter beside each line.",
  },
  {
    key: "editor.wordWrap",
    kind: "boolean",
    default: false,
    group: "Text",
    ticket: "EP-02",
    label: "Word wrap",
    description: "Wraps long lines so they remain inside the Editor window.",
  },
  {
    key: "editor.renderWhitespace",
    kind: "boolean",
    default: false,
    group: "Text",
    ticket: "EP-02",
    label: "Show whitespace",
    description: "Reveals spaces and tabs when indentation is unclear.",
  },
  {
    key: "editor.tabSize",
    kind: "integer",
    default: 2,
    min: 2,
    max: 8,
    choices: [2, 4, 8],
    unit: "spaces",
    group: "Editing",
    ticket: "EP-02",
    label: "Tab size",
    description: "Sets the number of spaces used for indentation.",
  },
  {
    key: "editor.matchBrackets",
    kind: "boolean",
    default: true,
    group: "Text",
    ticket: "EP-03",
    label: "Match brackets",
    description:
      "Highlights the bracket that pairs with the one at the cursor.",
  },
  {
    key: "editor.autoClosingBrackets",
    kind: "boolean",
    default: true,
    group: "Editing",
    ticket: "EP-03",
    label: "Auto-close brackets",
    description:
      "Types the closing bracket or quote when you type the opening one.",
  },
  {
    key: "editor.folding",
    kind: "boolean",
    default: true,
    group: "Text",
    ticket: "EP-03",
    label: "Code folding",
    description: "Shows fold arrows in the gutter so blocks can be collapsed.",
  },
  {
    key: "editor.occurrencesHighlight",
    kind: "boolean",
    default: true,
    group: "Text",
    ticket: "EP-03",
    label: "Highlight occurrences",
    description:
      "Marks other copies of the selected text or the word at the cursor.",
  },
  {
    key: "editor.multiCursorModifier",
    kind: "choice",
    default: "alt",
    choices: ["alt", "ctrlCmd"],
    choiceLabels: { alt: ["Alt", "Option"], ctrlCmd: ["Ctrl", "Cmd"] },
    group: "Editing",
    ticket: "EP-03",
    label: "Multi-cursor modifier",
    description: "The key to hold while clicking to add another cursor.",
  },
] as const satisfies readonly EditorSettingSpec[];

type Entry = (typeof EDITOR_SETTINGS)[number];

export type EditorSettingKey = Entry["key"];

export type EditorSettings = {
  [E in Entry as E["key"]]: E extends { kind: "boolean" }
    ? boolean
    : E extends { choices: readonly (infer Choice)[] }
      ? Choice
      : never;
};

export const DEFAULT_EDITOR_SETTINGS = Object.freeze(
  Object.fromEntries(
    EDITOR_SETTINGS.map((spec) => [spec.key, spec.default]),
  ) as EditorSettings,
);

/**
 * The value as the registry allows it, matching the Rust side: whole numbers
 * are clamped to the range, then moved to the nearest choice (the lower one on
 * a tie). A choice must be one of its names. A value of the wrong type is
 * `undefined`.
 */
export function admitEditorSetting(
  spec: EditorSettingSpec,
  value: unknown,
): boolean | number | string | undefined {
  if (spec.kind === "boolean") {
    return typeof value === "boolean" ? value : undefined;
  }
  if (spec.kind === "choice") {
    return typeof value === "string" && spec.choices.includes(value)
      ? value
      : undefined;
  }
  if (typeof value !== "number" || !Number.isSafeInteger(value)) {
    return undefined;
  }
  const clamped = Math.min(spec.max, Math.max(spec.min, value));
  let nearest = clamped;
  let distance = Infinity;
  for (const choice of spec.choices) {
    const gap = Math.abs(choice - clamped);
    if (gap < distance || (gap === distance && choice < nearest)) {
      nearest = choice;
      distance = gap;
    }
  }
  return nearest;
}

/**
 * Every registry key with an allowed value. Unknown keys are dropped, and a
 * missing or mistyped value falls back to its default.
 */
export function admitEditorSettings(input: unknown): EditorSettings {
  const record =
    typeof input === "object" && input !== null
      ? (input as Record<string, unknown>)
      : {};
  return Object.fromEntries(
    EDITOR_SETTINGS.map((spec) => [
      spec.key,
      admitEditorSetting(spec, record[spec.key]) ?? spec.default,
    ]),
  ) as EditorSettings;
}

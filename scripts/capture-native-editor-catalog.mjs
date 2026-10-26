import { validateProbe } from "./capture-native-chat-contract.mjs";

export const EDITOR_FILE = "src/features/outline/syntaxOutline.ts";
export const SEARCH_FILE = "src/features/outline/outlineCard.ts";
export const EDITOR_CATALOG = Object.freeze(
  [
    {
      state: "editor-idle",
      supported: true,
      reason: "One File, native caret and Explorer.",
      widths: [1888, 720, 480],
      absentWidths: {
        720: "Below 761px Explorer is a modal drawer; the Editor is inert and aria-hidden, so Explorer and a visible caret cannot coexist.",
        480: "Below 761px Explorer is a modal drawer; the Editor is inert and aria-hidden, so Explorer and a visible caret cannot coexist.",
      },
    },
    {
      state: "tabs-many",
      supported: false,
      reason:
        "File tabs can be dirty, but TabStrip and WorkbenchTab have no preview-tab lifecycle; eight permanent tabs are not this state.",
      widths: [1888],
    },
    {
      state: "split-editor",
      supported: false,
      reason:
        "Editor has one File / Diff / Settings strip and one ActiveEditorCanvas; split Editor groups violate AGENTS.md.",
      widths: [1888],
    },
    {
      state: "outline",
      supported: true,
      reason:
        "Native Outline activity view with a focused symbol button after verified caret navigation; not a VS Code Explorer subsection or persistent selected-symbol highlight.",
      widths: [1888, 720, 480],
    },
    {
      state: "find-widget",
      supported: false,
      reason:
        "Ctrl+F and a match counter exist, but FileFindBar / inFileFind support only literal find and Match case, not regex.",
      widths: [1888],
    },
    {
      state: "find-replace",
      supported: false,
      reason:
        "FileFindBar has no replace field or replace operation; the keymap has no Ctrl+H binding.",
      widths: [1888],
    },
    {
      state: "multi-cursor",
      supported: false,
      reason:
        "CodeMirror is configured without allowMultipleSelections or multi-selection commands; the EditorAdapter exposes a single start/end selection.",
      widths: [1888],
    },
    {
      state: "go-to-line",
      supported: true,
      reason: "Native Ctrl+G FileGoToLineBar prompt, input and status.",
      widths: [1888],
    },
    {
      state: "quick-open",
      supported: false,
      reason:
        "Ctrl+P is reservedFor EP-08 in keymapRegistry; no Quick Open listener or surface exists.",
      widths: [1888, 720, 480],
    },
    {
      state: "command-palette",
      supported: false,
      reason:
        "No command-palette surface or Ctrl+Shift+P / format command binding exists.",
      widths: [1888],
    },
    {
      state: "search-view",
      supported: true,
      reason:
        "Native Search tool with results in two real repository files and an expanded result group.",
      widths: [1888],
    },
    {
      state: "settings-editor",
      supported: true,
      reason:
        "Native Settings Editor page (Sage calls the Text Editor section Editor).",
      widths: [1888],
    },
  ].map((entry) =>
    Object.freeze({
      ...entry,
      widths: Object.freeze(entry.widths),
      absentWidths: Object.freeze(entry.absentWidths ?? {}),
    }),
  ),
);

export function editorCell(state, width) {
  const entry = EDITOR_CATALOG.find((item) => item.state === state);
  if (!entry || !entry.widths.includes(width))
    throw new Error("editor-state-width");
  return {
    harness: "editor",
    state,
    width,
    supported: entry.supported && !entry.absentWidths[width],
    reason: entry.absentWidths[width] ?? entry.reason,
  };
}
export function editorCells() {
  return EDITOR_CATALOG.flatMap(({ state, widths }) =>
    widths.map((width) => editorCell(state, width)),
  );
}
export function admitEditor(options) {
  const cell = editorCell(options.state, options.width);
  if (!cell.supported) throw new Error("truthful-absent-state");
  if (
    options.harness !== "editor" ||
    options.caseId !== "editor-" + cell.state ||
    options.height !== (cell.width === 1888 ? 1096 : 1000)
  )
    throw new Error("case-identity");
  return options;
}
export function parseEditorArgs(args) {
  const values = {};
  if (args.length !== 6) throw new Error("arguments");
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index];
    if (!["--harness", "--state", "--width"].includes(key) || key in values)
      throw new Error("arguments");
    values[key] = args[index + 1];
  }
  const width = Number(values["--width"]);
  return admitEditor({
    harness: values["--harness"],
    state: values["--state"],
    width,
    height: width === 1888 ? 1096 : 1000,
    caseId: "editor-" + values["--state"],
  });
}
export const STATE_WITNESSES = Object.freeze({
  "editor-idle": [
    "file",
    "oneTab",
    "caretMidFile",
    "editorFocused",
    "explorer",
  ],
  outline: [
    "file",
    "oneTab",
    "outlineExpanded",
    "symbolFocused",
    "symbolCaret",
  ],
  "go-to-line": ["file", "oneTab", "prompt", "promptFocused", "promptStatus"],
  "search-view": [
    "file",
    "oneTab",
    "search",
    "searchQuery",
    "multipleFiles",
    "expandedResults",
  ],
  "settings-editor": [
    "settings",
    "settingsTab",
    "editorPage",
    "settingsControls",
  ],
});
export function validateEditorProbe(probe) {
  return validateProbe(probe, (value) => {
    const state = value.caseId?.replace(/^editor-/u, "");
    admitEditor({
      harness: "editor",
      state,
      width: value.dimensions?.client?.width,
      height: value.dimensions?.client?.height,
      caseId: value.caseId,
    });
    if (
      value.bindingId !== "native-editor" ||
      value.protocol !== "native-editor-1" ||
      value.turnId !== "none" ||
      value.assistantRendered !== false
    )
      throw new Error("probe-identity");
    const names = [
      "namedState",
      "binding",
      "editorMode",
      "noAgentChrome",
      ...STATE_WITNESSES[state],
    ];
    if (
      Object.keys(value.predicates ?? {})
        .sort()
        .join() !== names.sort().join() ||
      Object.values(value.predicates).some((flag) => flag !== true)
    )
      throw new Error("state-predicate");
  });
}

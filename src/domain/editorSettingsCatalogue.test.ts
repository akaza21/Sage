import { readFileSync } from "node:fs";
import path from "node:path";

import { describe, expect, it } from "vitest";

import { editorAppearanceFrom } from "../app/editorSettingsStore";
import { DEFAULT_EDITOR_APPEARANCE } from "../features/files/editorAppearance";
import {
  admitEditorSetting,
  admitEditorSettings,
  DEFAULT_EDITOR_SETTINGS,
  EDITOR_SETTINGS,
  type EditorSettingSpec,
  MAX_GLOBS,
  sameEditorSettingValue,
} from "./editorSettingsCatalogue";
import {
  EDITOR_SETTING_COPY,
  EDITOR_SETTING_GROUPS,
} from "./editorSettingsCopy";
import { MAX_PATH_GLOB_BYTES } from "./pathGlob";

const spec = (key: string): EditorSettingSpec => {
  const found = EDITOR_SETTINGS.find((entry) => entry.key === key);
  if (!found) throw new Error(`no ${key}`);
  return found;
};

/**
 * The cases the Rust store's own test also runs: the values it sends to the
 * webview, the writes it admits and the writes it refuses.
 */
interface NativeSettingsCases {
  defaults: Record<string, unknown>;
  writes: {
    changes: Record<string, unknown>;
    stored: Record<string, unknown>;
  }[];
  rejected: Record<string, unknown>[];
}

const nativeCases = JSON.parse(
  readFileSync(
    path.resolve(
      __dirname,
      "../../src-tauri/src/settings_store/editor_settings_cases.json",
    ),
    "utf8",
  ),
) as NativeSettingsCases;

const catalogueSpec = (key: string) =>
  EDITOR_SETTINGS.find((entry) => entry.key === key);

describe("editor settings catalogue", () => {
  it("reads the native store's defaults as its own and shows them as the default appearance", () => {
    expect(Object.keys(nativeCases.defaults).sort()).toEqual(
      EDITOR_SETTINGS.map((entry) => entry.key).sort(),
    );
    const settings = admitEditorSettings(nativeCases.defaults);
    expect(settings).toEqual(nativeCases.defaults);
    expect(settings).toEqual(DEFAULT_EDITOR_SETTINGS);
    expect(editorAppearanceFrom(settings)).toEqual(DEFAULT_EDITOR_APPEARANCE);
  });

  it("admits and refuses the same writes as the native store and reads its answer unchanged", () => {
    const written = new Set<string>();
    for (const { changes, stored } of nativeCases.writes) {
      for (const [key, value] of Object.entries(changes)) {
        const entry = catalogueSpec(key);
        if (!entry) {
          expect(stored).not.toHaveProperty(key);
          continue;
        }
        written.add(key);
        expect(admitEditorSetting(entry, value), key).toEqual(stored[key]);
      }
      const answer = { ...nativeCases.defaults, ...stored };
      const settings = admitEditorSettings(answer);
      expect(settings).toEqual(answer);
      expect(editorAppearanceFrom(settings)).toEqual({
        fontSize: answer["editor.fontSize"],
        tabSize: answer["editor.tabSize"],
        wordWrap: answer["editor.wordWrap"],
        lineNumbers: answer["editor.lineNumbers"],
        showWhitespace: answer["editor.renderWhitespace"],
      });
    }
    expect([...written].sort()).toEqual(
      EDITOR_SETTINGS.map((entry) => entry.key).sort(),
    );
    for (const change of nativeCases.rejected) {
      for (const [key, value] of Object.entries(change)) {
        expect(admitEditorSetting(spec(key), value), key).toBeUndefined();
      }
    }
  });

  it("gives every entry a unique key, words in a known group and a default it admits", () => {
    expect(new Set(EDITOR_SETTINGS.map((entry) => entry.key)).size).toBe(
      EDITOR_SETTINGS.length,
    );
    expect(Object.keys(EDITOR_SETTING_COPY).sort()).toEqual(
      EDITOR_SETTINGS.map((entry) => entry.key).sort(),
    );
    for (const entry of EDITOR_SETTINGS) {
      const copy = EDITOR_SETTING_COPY[entry.key];
      expect(EDITOR_SETTING_GROUPS).toContain(copy.group);
      expect(admitEditorSetting(entry, entry.default)).toEqual(entry.default);
      expect(copy.label).not.toBe("");
      expect(copy.description).not.toBe("");
      expect(copy.unit !== undefined).toBe(entry.kind === "integer");
      expect(copy.choiceLabels !== undefined).toBe(entry.kind === "choice");
      if (entry.kind === "choice") {
        for (const choice of entry.choices) {
          expect(copy.choiceLabels?.[choice]).toHaveLength(2);
        }
      }
    }
  });

  it("clamps numbers to the range and the nearest choice, like the Rust store", () => {
    const fontSize = spec("editor.fontSize");
    const tabSize = spec("editor.tabSize");
    expect(
      [15, 99, -3, 13].map((value) => admitEditorSetting(fontSize, value)),
    ).toEqual([14, 16, 12, 13]);
    expect(
      [3, 6, 7, 100].map((value) => admitEditorSetting(tabSize, value)),
    ).toEqual([2, 4, 8, 8]);
  });

  it("keeps the runtime's written-out defaults equal to the catalogue's", () => {
    expect(editorAppearanceFrom(DEFAULT_EDITOR_SETTINGS)).toEqual(
      DEFAULT_EDITOR_APPEARANCE,
    );
  });

  it("refuses values of the wrong type", () => {
    expect(admitEditorSetting(spec("editor.fontSize"), "14")).toBeUndefined();
    expect(admitEditorSetting(spec("editor.fontSize"), 13.5)).toBeUndefined();
    expect(
      admitEditorSetting(spec("editor.fontSize"), 2 ** 53),
    ).toBeUndefined();
    expect(
      admitEditorSetting(spec("editor.fontSize"), Number.NaN),
    ).toBeUndefined();
    expect(admitEditorSetting(spec("editor.wordWrap"), 1)).toBeUndefined();
    const modifier = spec("editor.multiCursorModifier");
    expect(admitEditorSetting(modifier, "ctrlCmd")).toBe("ctrlCmd");
    expect(admitEditorSetting(modifier, "meta")).toBeUndefined();
    expect(admitEditorSetting(modifier, 1)).toBeUndefined();
  });

  it("admits a bounded list of valid path globs, like the Rust store", () => {
    const exclude = spec("files.exclude");
    expect(admitEditorSetting(exclude, ["**/dist", "*.log"])).toEqual([
      "**/dist",
      "*.log",
    ]);
    expect(admitEditorSetting(exclude, [])).toEqual([]);
    expect(admitEditorSetting(exclude, ["*.log", "**/dist", "*.log"])).toEqual([
      "*.log",
      "**/dist",
    ]);
    // A lone surrogate would fail on its way to Rust; a paired one is fine.
    expect(admitEditorSetting(exclude, ["a\uD800b"])).toBeUndefined();
    expect(admitEditorSetting(exclude, ["**/\u{1F600}"])).toEqual([
      "**/\u{1F600}",
    ]);
    const full = Array.from({ length: MAX_GLOBS }, (_, index) => `p${index}`);
    expect(admitEditorSetting(exclude, full)).toEqual(full);
    for (const bad of [
      "**/dist",
      [""],
      ["a".repeat(MAX_PATH_GLOB_BYTES + 1)],
      ["é".repeat(MAX_PATH_GLOB_BYTES / 2 + 1)],
      ["tab\there"],
      ["\u0085"],
      ["\uDC00"],
      ["/absolute"],
      ["dist/"],
      ["../outside"],
      [1],
      [...full, "one more"],
      null,
    ]) {
      expect(admitEditorSetting(exclude, bad)).toBeUndefined();
    }
  });

  it("keeps an admitted list's identity and compares lists by their patterns", () => {
    const admitted = admitEditorSettings({ "files.exclude": ["**/dist"] });
    expect(admitEditorSettings(admitted)["files.exclude"]).toBe(
      admitted["files.exclude"],
    );
    expect(admitEditorSettings(null)["files.exclude"]).toBe(
      DEFAULT_EDITOR_SETTINGS["files.exclude"],
    );
    expect(sameEditorSettingValue(["a", "b"], ["a", "b"])).toBe(true);
    expect(sameEditorSettingValue(["a", "b"], ["b", "a"])).toBe(false);
    expect(sameEditorSettingValue(["a"], "a")).toBe(false);
    expect(sameEditorSettingValue(2, 2)).toBe(true);
  });

  it("admits a whole record: unknown keys dropped, bad values defaulted", () => {
    expect(admitEditorSettings(null)).toEqual(DEFAULT_EDITOR_SETTINGS);
    expect(
      admitEditorSettings({
        "editor.fontSize": 16,
        "editor.wordWrap": "on",
        "editor.unknown": true,
      }),
    ).toEqual({ ...DEFAULT_EDITOR_SETTINGS, "editor.fontSize": 16 });
  });
});

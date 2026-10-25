import { readFileSync } from "node:fs";
import path from "node:path";

import { describe, expect, it } from "vitest";

import { editorAppearanceFrom } from "../app/editorSettingsStore";
import { DEFAULT_EDITOR_APPEARANCE } from "../features/files/editorAppearance";
import {
  admitEditorSetting,
  admitEditorSettings,
  DEFAULT_EDITOR_SETTINGS,
  EDITOR_SETTING_GROUPS,
  EDITOR_SETTINGS,
  type EditorSettingSpec,
} from "./editorSettingsCatalogue";

const spec = (key: string): EditorSettingSpec => {
  const found = EDITOR_SETTINGS.find((entry) => entry.key === key);
  if (!found) throw new Error(`no ${key}`);
  return found;
};

/** The comparable part of an entry: what Rust also stores. */
interface StoredShape {
  key: string;
  kind: "boolean" | "integer";
  default: boolean | number;
  min?: number;
  max?: number;
  choices?: number[];
  ticket: string;
}

function rustRegistry(): StoredShape[] {
  const source = readFileSync(
    path.resolve(
      __dirname,
      "../../src-tauri/src/settings_store/editor_settings.rs",
    ),
    "utf8",
  );
  const block =
    /pub const EDITOR_SETTINGS: &\[SettingSpec\] = &\[([\s\S]*?)\n\];/u.exec(
      source,
    )?.[1];
  if (!block) throw new Error("EDITOR_SETTINGS not found in the Rust store");
  return block
    .split("SettingSpec {")
    .slice(1)
    .map((entry) => {
      const key = /key: "([^"]+)"/u.exec(entry)?.[1];
      const ticket = /ticket: "([^"]+)"/u.exec(entry)?.[1];
      const flag = /default: SettingValue::Bool\((true|false)\)/u.exec(entry);
      const number = /default: SettingValue::Int\((-?\d+)\)/u.exec(entry);
      if (!key || !ticket || (!flag && !number)) {
        throw new Error(`unreadable Rust entry: ${entry}`);
      }
      if (/kind: SettingKind::Bool/u.test(entry)) {
        return { key, kind: "boolean", default: flag?.[1] === "true", ticket };
      }
      const range =
        /kind: SettingKind::Int \{\s*min: (-?\d+),\s*max: (-?\d+),\s*choices: &\[([^\]]*)\],?\s*\}/u.exec(
          entry,
        );
      if (!range) throw new Error(`unreadable Rust kind for ${key}`);
      return {
        key,
        kind: "integer",
        default: Number(number?.[1]),
        min: Number(range[1]),
        max: Number(range[2]),
        choices: range[3]
          .split(",")
          .map((choice) => choice.trim())
          .filter(Boolean)
          .map(Number),
        ticket,
      };
    });
}

function storedShape(entry: EditorSettingSpec): StoredShape {
  return entry.kind === "boolean"
    ? {
        key: entry.key,
        kind: entry.kind,
        default: entry.default,
        ticket: entry.ticket,
      }
    : {
        key: entry.key,
        kind: entry.kind,
        default: entry.default,
        min: entry.min,
        max: entry.max,
        choices: [...entry.choices],
        ticket: entry.ticket,
      };
}

describe("editor settings catalogue", () => {
  it("matches the Rust registry key for key, in order", () => {
    const rust = rustRegistry();
    expect(rust.length).toBeGreaterThan(0);
    expect(EDITOR_SETTINGS.map(storedShape)).toEqual(rust);
  });

  it("gives every entry a unique key, a known group and a default it admits", () => {
    expect(new Set(EDITOR_SETTINGS.map((entry) => entry.key)).size).toBe(
      EDITOR_SETTINGS.length,
    );
    for (const entry of EDITOR_SETTINGS) {
      expect(EDITOR_SETTING_GROUPS).toContain(entry.group);
      expect(admitEditorSetting(entry, entry.default)).toBe(entry.default);
      expect(entry.label).not.toBe("");
      expect(entry.description).not.toBe("");
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

import { describe, expect, it } from "vitest";

import {
  SETTINGS_PAGES,
  admitSettingsPageId,
  matchingSettingsEntryIds,
  pagesWithMatches,
} from "./settingsNavigation";

describe("settings navigation", () => {
  it("orders the pages so Editor comes before Keyboard", () => {
    expect(SETTINGS_PAGES.map((page) => page.id)).toEqual([
      "editor",
      "keyboard",
    ]);
  });

  it("admits a known page id and falls back to the first page otherwise", () => {
    expect(admitSettingsPageId("keyboard")).toBe("keyboard");
    expect(admitSettingsPageId("nope")).toBe("editor");
    expect(admitSettingsPageId(null)).toBe("editor");
  });

  it("treats a blank query as matching every entry on a page", () => {
    const ids = matchingSettingsEntryIds("   ", "editor");
    expect(ids).toEqual(
      new Set(
        SETTINGS_PAGES.find((page) => page.id === "editor")?.entries.map(
          (entry) => entry.id,
        ),
      ),
    );
  });

  it("matches an entry on its label, description and keywords", () => {
    expect(matchingSettingsEntryIds("tab size", "editor")).toContain(
      "editor.tabSize",
    );
    expect(matchingSettingsEntryIds("indentation", "editor")).toContain(
      "editor.tabSize",
    );
    expect(matchingSettingsEntryIds("TAB SIZE", "editor")).toContain(
      "editor.tabSize",
    );
  });

  it("excludes entries the query does not describe", () => {
    expect(matchingSettingsEntryIds("tab size", "editor")).not.toContain(
      "editor.wordWrap",
    );
  });

  it("reports which pages still have matches so search spans the nav", () => {
    expect(pagesWithMatches("tab size")).toEqual(new Set(["editor"]));
    expect(pagesWithMatches("")).toEqual(
      new Set(SETTINGS_PAGES.map((page) => page.id)),
    );
  });

  it("reports no pages when nothing anywhere matches", () => {
    expect(pagesWithMatches("zzzznomatch")).toEqual(new Set());
  });

  it("finds the save shortcut from the Keyboard page", () => {
    expect(matchingSettingsEntryIds("save", "keyboard")).toContain("save");
  });
});

describe("reserved shortcuts", () => {
  it("leaves a reserved binding off the Keyboard page until its ticket lands", () => {
    expect(matchingSettingsEntryIds("quick open", "keyboard")).toEqual(
      new Set(),
    );
    expect(matchingSettingsEntryIds("", "keyboard")).not.toContain("quickOpen");
  });
});

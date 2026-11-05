import { describe, expect, it } from "vitest";

import {
  SETTINGS_PAGES,
  admitSettingsPageId,
  matchingSettingsEntryIds,
  pagesWithMatches,
} from "./settingsNavigation";

describe("settings navigation", () => {
  it("orders the pages Editor, Keyboard, Browser, then Passwords", () => {
    expect(SETTINGS_PAGES.map((page) => page.id)).toEqual([
      "editor",
      "keyboard",
      "browser",
      "passwords",
    ]);
  });

  it("finds the Passwords page by import and saved-password searches", () => {
    expect(admitSettingsPageId("passwords")).toBe("passwords");
    expect(pagesWithMatches("csv")).toEqual(new Set(["passwords"]));
    expect(matchingSettingsEntryIds("delete all", "passwords")).toEqual(
      new Set(["passwords.deleteAll"]),
    );
  });

  it("finds the Browser settings by their Codex labels", () => {
    expect(admitSettingsPageId("browser")).toBe("browser");
    expect(pagesWithMatches("full url")).toEqual(new Set(["browser"]));
    expect(matchingSettingsEntryIds("address bar", "browser")).toEqual(
      new Set(["browser.showFullUrl"]),
    );
    expect(matchingSettingsEntryIds("cache", "browser")).toEqual(
      new Set(["browser.clearData"]),
    );
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

  it("keeps Agent shortcuts off the Editor's Keyboard page", () => {
    const ids = matchingSettingsEntryIds("", "keyboard");
    expect(ids).toContain("openSettings");
    expect(ids).not.toContain("chat.findInChat");
    expect(ids).not.toContain("sendMessage");
  });
});

describe("Quick Open shortcut", () => {
  it("is listed on the Keyboard page now that Quick Open exists", () => {
    expect(matchingSettingsEntryIds("quick open", "keyboard")).toContain(
      "quickOpen",
    );
  });
});

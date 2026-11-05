// Settings pages that both Settings surfaces show, Editor Settings and the
// Agent settings panel. Each loads once, as one lazy chunk, whichever
// surface opens it first.

export const loadSettingsBrowserPage = () =>
  import("./SettingsBrowserPage").then(({ SettingsBrowserPage }) => ({
    default: SettingsBrowserPage,
  }));

export const loadSettingsPasswordsPage = () =>
  import("./SettingsPasswordsPage").then(({ SettingsPasswordsPage }) => ({
    default: SettingsPasswordsPage,
  }));

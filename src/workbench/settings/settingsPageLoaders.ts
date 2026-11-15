export const loadSettingsBrowserPage = () =>
  import("./SettingsBrowserPage").then(({ SettingsBrowserPage }) => ({
    default: SettingsBrowserPage,
  }));

export const loadSettingsPasswordsPage = () =>
  import("./SettingsPasswordsPage").then(({ SettingsPasswordsPage }) => ({
    default: SettingsPasswordsPage,
  }));

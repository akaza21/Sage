import type * as NewTabModule from "./BrowserNewTab";
// Chromium remembers a rejected module URL. An explicit retry has a separate,
// bounded module identity; it still renders this tab and never opens another.
const recoveryNewTab = import.meta.glob<typeof NewTabModule>(
  "./BrowserNewTab.tsx",
  { query: "?sage-browser-recovery" },
);
let newTabImportFailed = false;
export const loadBrowserNewTab = async () => {
  try {
    const module = await (newTabImportFailed
      ? recoveryNewTab["./BrowserNewTab.tsx"]()
      : import("./BrowserNewTab"));
    return { default: module.BrowserNewTab };
  } catch (cause) {
    newTabImportFailed = true;
    throw cause;
  }
};

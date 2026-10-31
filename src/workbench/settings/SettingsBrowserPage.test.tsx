import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

const getPreferences = vi.fn();
const setPreferences = vi.fn();
const getDeveloperMode = vi.fn();
const setDeveloperMode = vi.fn();
const runtime = vi.hoisted(() => ({ desktop: true }));

vi.mock("../../ports/nativeInvoke", () => ({
  isTauriRuntime: () => runtime.desktop,
}));
vi.mock("../../ports/nativeBrowserPreferencesPort", () => ({
  browserPreferences: () => getPreferences(),
  browserSetPreferences: (value: unknown) => setPreferences(value),
  browserDeveloperMode: () => getDeveloperMode(),
  browserSetDeveloperMode: (enabled: boolean) => setDeveloperMode(enabled),
}));
vi.mock("../../ports/nativeBrowserPort", () => ({
  browserClearData: vi.fn(),
  closeBrowser: vi.fn(),
}));

import { resetBrowserPreferencesForTests } from "../../features/browser/browserPreferencesStore";
import { matchingSettingsEntryIds } from "../../domain/settingsNavigation";
import { SettingsBrowserPage } from "./SettingsBrowserPage";

const all = () => matchingSettingsEntryIds("", "browser");

beforeEach(() => {
  vi.clearAllMocks();
  runtime.desktop = true;
  getPreferences.mockResolvedValue({ showFullUrl: false });
  setPreferences.mockImplementation(async (value: unknown) => value);
  getDeveloperMode.mockResolvedValue({ available: false, enabled: false });
  setDeveloperMode.mockImplementation(async (enabled: boolean) => ({
    available: true,
    enabled,
  }));
  resetBrowserPreferencesForTests();
});

afterEach(cleanup);

it("lists the General settings that Sage's browser supports", async () => {
  render(<SettingsBrowserPage visible={all()} />);
  expect(screen.getByRole("heading", { name: "Browser" })).toBeVisible();
  const general = screen.getByRole("group", { name: "General" });
  const toggle = await screen.findByRole("checkbox", { name: "Show full URL" });
  expect(general).toContainElement(toggle);
  expect(toggle).toHaveAccessibleDescription(
    "Include the path, query, and fragment in the address bar. Default: Off",
  );
  const clear = screen.getByRole("button", { name: "Clear browsing data" });
  expect(clear).toBeEnabled();
  expect(clear).toHaveAccessibleDescription(
    "Clear browsing history, cookies, site data, and cache from the built-in browser. Every browser tab closes.",
  );
  expect(general).toHaveTextContent("Browsing data");
});

it("loads the stored choice and saves a change through Rust", async () => {
  getPreferences.mockResolvedValue({ showFullUrl: true });
  render(<SettingsBrowserPage visible={all()} />);
  const toggle = await screen.findByRole("checkbox", { name: "Show full URL" });
  await vi.waitFor(() => expect(toggle).toBeChecked());
  expect(toggle).toBeEnabled();
  fireEvent.click(toggle);
  expect(toggle).not.toBeChecked();
  expect(setPreferences).toHaveBeenCalledWith({ showFullUrl: false });
});

it("keeps the toggle disabled and says why when loading fails", async () => {
  getPreferences.mockRejectedValue(new Error("unavailable"));
  render(<SettingsBrowserPage visible={all()} />);
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Browser settings could not be loaded.",
  );
  expect(
    screen.getByRole("checkbox", { name: "Show full URL" }),
  ).toBeDisabled();
  getPreferences.mockResolvedValue({ showFullUrl: true });
  fireEvent.click(screen.getByRole("button", { name: "Retry" }));
  await vi.waitFor(() =>
    expect(
      screen.getByRole("checkbox", { name: "Show full URL" }),
    ).toBeChecked(),
  );
  expect(screen.queryByRole("alert")).toBeNull();
});

it("hides entries the search filtered out", () => {
  render(
    <SettingsBrowserPage
      visible={matchingSettingsEntryIds("cookies", "browser")}
    />,
  );
  expect(screen.queryByRole("checkbox", { name: "Show full URL" })).toBeNull();
  expect(
    screen.getByRole("button", { name: "Clear browsing data" }),
  ).toBeVisible();
});

it("explains that browser settings need the desktop app", () => {
  runtime.desktop = false;
  render(<SettingsBrowserPage visible={all()} />);
  expect(screen.getByRole("status")).toHaveTextContent(
    "Browser settings are available in the Sage desktop app.",
  );
  expect(screen.queryByRole("checkbox")).toBeNull();
  expect(getPreferences).not.toHaveBeenCalled();
});

it("offers developer mode off by default, names its risk, and saves through Rust", async () => {
  getDeveloperMode.mockResolvedValue({ available: true, enabled: false });
  render(<SettingsBrowserPage visible={all()} />);
  const group = await screen.findByRole("group", { name: "Developer mode" });
  const toggle = screen.getByRole("checkbox", {
    name: "Enable full CDP access",
  });
  expect(group).toContainElement(toggle);
  expect(group).toHaveTextContent("Elevated risk");
  expect(toggle).not.toBeChecked();
  expect(toggle).toHaveAccessibleDescription(
    /run scripts on the page and read the site's cookies and session data.*Default: Off$/,
  );
  fireEvent.click(toggle);
  expect(setDeveloperMode).toHaveBeenCalledWith(true);
  await vi.waitFor(() => expect(toggle).toBeChecked());
  fireEvent.click(toggle);
  expect(setDeveloperMode).toHaveBeenLastCalledWith(false);
  await vi.waitFor(() => expect(toggle).not.toBeChecked());
});

it("hides developer mode where the browser has no CDP", async () => {
  render(<SettingsBrowserPage visible={all()} />);
  await screen.findByRole("checkbox", { name: "Show full URL" });
  await vi.waitFor(() => expect(getDeveloperMode).toHaveBeenCalled());
  expect(screen.queryByRole("group", { name: "Developer mode" })).toBeNull();
});

it("says so when developer mode cannot be changed, and keeps Rust's value", async () => {
  getDeveloperMode.mockResolvedValue({ available: true, enabled: false });
  setDeveloperMode.mockRejectedValue(new Error("unavailable"));
  render(<SettingsBrowserPage visible={all()} />);
  const toggle = await screen.findByRole("checkbox", {
    name: "Enable full CDP access",
  });
  fireEvent.click(toggle);
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Couldn't change developer mode. Try again.",
  );
  expect(toggle).not.toBeChecked();
});

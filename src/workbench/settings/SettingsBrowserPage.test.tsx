import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

const getPreferences = vi.fn();
const setPreferences = vi.fn();
const runtime = vi.hoisted(() => ({ desktop: true }));

vi.mock("../../ports/nativeInvoke", () => ({
  isTauriRuntime: () => runtime.desktop,
}));
vi.mock("../../ports/nativeBrowserPreferencesPort", () => ({
  browserPreferences: () => getPreferences(),
  browserSetPreferences: (value: unknown) => setPreferences(value),
}));
vi.mock("../../ports/nativeBrowserDownloadFolderPort", async (actual) => ({
  ...(await actual<Record<string, unknown>>()),
  browserDownloadFolder: async () => ({ path: null, available: false }),
  browserChooseDownloadFolder: vi.fn(),
  browserResetDownloadFolder: vi.fn(),
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

it("lists the download location under its own Downloads group", async () => {
  render(<SettingsBrowserPage visible={all()} />);
  const downloads = screen.getByRole("group", { name: "Downloads" });
  expect(downloads).toHaveTextContent("Download location");
  expect(
    await screen.findByRole("button", { name: "Change…" }),
  ).toBeInTheDocument();
});

it("hides the Downloads group outside the desktop app", () => {
  runtime.desktop = false;
  render(<SettingsBrowserPage visible={all()} />);
  expect(screen.queryByRole("group", { name: "Downloads" })).toBeNull();
});

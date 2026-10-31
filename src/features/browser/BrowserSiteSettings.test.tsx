import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { listen } from "@tauri-apps/api/event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

const list = vi.fn();
const set = vi.fn();
const reset = vi.fn();
const resetAll = vi.fn();
const reloadTabs = vi.fn();

vi.mock("../../ports/nativeBrowserSiteSettingsPort", () => ({
  browserSiteSettings: () => list(),
  browserSiteSettingSet: (origin: string, kind: string, decision: string) =>
    set(origin, kind, decision),
  browserSiteSettingReset: (origin: string, kind: string) =>
    reset(origin, kind),
  browserSiteSettingsResetAll: () => resetAll(),
  browserSiteReloadTabs: (origins: string[]) => reloadTabs(origins),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => undefined),
}));

import { BrowserSiteSettings } from "./BrowserSiteSettings";
import {
  admitBrowserSiteSettings,
  admitSiteSettingsUpdate,
  MAX_SITE_SETTINGS,
} from "./browserSiteSettings";

const camera = {
  origin: "https://a.example",
  kind: "camera",
  decision: "allow",
} as const;
const location = {
  origin: "https://b.example:8443",
  kind: "geolocation",
  decision: "deny",
} as const;

beforeEach(() => {
  vi.mocked(listen).mockReset();
  vi.mocked(listen).mockResolvedValue(() => undefined);
  for (const mock of [list, set, reset, resetAll, reloadTabs]) mock.mockReset();
  list.mockResolvedValue([camera, location]);
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

function view() {
  render(
    <>
      <p id="note">Saved answers</p>
      <BrowserSiteSettings describedBy="note" />
    </>,
  );
}

it("lists every saved site answer with its setting", async () => {
  view();
  const table = await screen.findByRole("table");
  expect(table).toHaveAccessibleDescription("Saved answers");
  const rows = within(table).getAllByRole("row").slice(1);
  expect(rows).toHaveLength(2);
  expect(rows[0]).toHaveTextContent("https://a.example");
  expect(rows[0]).toHaveTextContent("Camera");
  expect(
    screen.getByRole("combobox", { name: "Camera for https://a.example" }),
  ).toHaveValue("allow");
  expect(
    screen.getByRole("combobox", {
      name: "Location for https://b.example:8443",
    }),
  ).toHaveValue("deny");
});

it("changes a site's answer through Rust and shows the stored list", async () => {
  set.mockResolvedValue({
    settings: [{ ...camera, decision: "deny" }, location],
    reloadFailed: [],
  });
  view();
  const select = await screen.findByRole("combobox", {
    name: "Camera for https://a.example",
  });
  fireEvent.change(select, { target: { value: "deny" } });
  expect(set).toHaveBeenCalledWith("https://a.example", "camera", "deny");
  await vi.waitFor(() => expect(select).toHaveValue("deny"));
});

it("resets one answer so the site asks again", async () => {
  reset.mockResolvedValue({ settings: [location], reloadFailed: [] });
  view();
  fireEvent.click(
    await screen.findByRole("button", {
      name: "Reset Camera for https://a.example",
    }),
  );
  expect(reset).toHaveBeenCalledWith("https://a.example", "camera");
  await vi.waitFor(() =>
    expect(
      screen.queryByRole("combobox", { name: "Camera for https://a.example" }),
    ).toBeNull(),
  );
});

it("resets every answer only after confirmation", async () => {
  const confirm = vi.spyOn(window, "confirm").mockReturnValueOnce(false);
  resetAll.mockResolvedValue({ settings: [], reloadFailed: [] });
  view();
  const button = await screen.findByRole("button", {
    name: "Reset all site permissions",
  });
  fireEvent.click(button);
  expect(resetAll).not.toHaveBeenCalled();
  confirm.mockReturnValueOnce(true);
  fireEvent.click(button);
  expect(resetAll).toHaveBeenCalledOnce();
  expect(
    await screen.findByText("No site permissions saved yet."),
  ).toBeVisible();
});

it("keeps a saved Block distinct from a failed reload and reloads again", async () => {
  set.mockResolvedValue({
    settings: [{ ...camera, decision: "deny" }, location],
    reloadFailed: ["https://a.example"],
  });
  reloadTabs
    .mockResolvedValueOnce(["https://a.example"])
    .mockResolvedValueOnce([]);
  view();
  const select = await screen.findByRole("combobox", {
    name: "Camera for https://a.example",
  });
  fireEvent.change(select, { target: { value: "deny" } });
  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent(
    "Saved, but tabs on https://a.example could not reload and may keep access until they do.",
  );
  expect(alert).not.toHaveTextContent("could not be changed");
  expect(select).toHaveValue("deny");
  const again = within(alert).getByRole("button", { name: "Reload tabs" });
  fireEvent.click(again);
  await vi.waitFor(() =>
    expect(reloadTabs).toHaveBeenCalledWith(["https://a.example"]),
  );
  await vi.waitFor(() => expect(again).toBeEnabled());
  expect(screen.getByRole("alert")).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Reload tabs" }));
  await vi.waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  expect(reloadTabs).toHaveBeenCalledTimes(2);
});

it("subscribes before the first read so a change in between is kept", async () => {
  let registered!: () => void;
  let deliver!: (payload: unknown) => void;
  vi.mocked(listen).mockImplementationOnce(
    (_event, handler) =>
      new Promise((resolve) => {
        deliver = (payload) =>
          handler({ event: "browser:site-settings", id: 1, payload });
        registered = () => resolve(() => undefined);
      }),
  );
  let answer!: (rows: unknown) => void;
  list.mockReturnValueOnce(new Promise((resolve) => (answer = resolve)));
  view();
  await vi.waitFor(() => expect(registered).toBeTypeOf("function"));
  expect(list).not.toHaveBeenCalled();
  await act(async () => registered());
  await vi.waitFor(() => expect(list).toHaveBeenCalledOnce());
  act(() => deliver([location]));
  await act(async () => answer([camera, location]));
  expect(
    screen.queryByRole("combobox", { name: "Camera for https://a.example" }),
  ).toBeNull();
  expect(
    screen.getByRole("combobox", {
      name: "Location for https://b.example:8443",
    }),
  ).toBeVisible();
});

it("says when nothing is saved", async () => {
  list.mockResolvedValue([]);
  view();
  expect(
    await screen.findByText("No site permissions saved yet."),
  ).toBeVisible();
  expect(screen.queryByRole("table")).toBeNull();
});

it("keeps the list and reports a failed save", async () => {
  set.mockRejectedValue(new Error("Sage keeps at most 256 site permissions"));
  view();
  const select = await screen.findByRole("combobox", {
    name: "Camera for https://a.example",
  });
  fireEvent.change(select, { target: { value: "deny" } });
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "The site permission could not be changed.",
  );
  expect(select).toHaveValue("allow");
  expect(select).toBeEnabled();
});

it("offers Retry when loading fails", async () => {
  list.mockRejectedValueOnce(new Error("unavailable"));
  view();
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Site permissions could not be loaded.",
  );
  fireEvent.click(screen.getByRole("button", { name: "Retry" }));
  expect(await screen.findByRole("table")).toBeVisible();
  expect(screen.queryByRole("alert")).toBeNull();
});

it("follows changes made from a browser tab", async () => {
  let deliver!: (payload: unknown) => void;
  vi.mocked(listen).mockImplementationOnce(async (_event, handler) => {
    deliver = (payload) =>
      handler({ event: "browser:site-settings", id: 1, payload });
    return () => undefined;
  });
  view();
  await screen.findByRole("table");
  await vi.waitFor(() => expect(deliver).toBeTypeOf("function"));
  act(() => deliver([location]));
  expect(
    screen.queryByRole("combobox", { name: "Camera for https://a.example" }),
  ).toBeNull();
  expect(
    screen.getByRole("combobox", {
      name: "Location for https://b.example:8443",
    }),
  ).toBeVisible();
});

it("admits only well-formed rows, bounded to the store's cap", () => {
  expect(
    admitBrowserSiteSettings([
      camera,
      { ...camera, kind: "usb" },
      { ...camera, decision: "ask" },
      { ...camera, origin: "https://a.example/path" },
      { ...camera, origin: "javascript:alert(1)" },
      { ...camera, origin: 7 },
      null,
    ]),
  ).toEqual([camera]);
  expect(admitBrowserSiteSettings({ rows: [camera] })).toEqual([]);
  expect(
    admitSiteSettingsUpdate({
      settings: [camera, { ...camera, kind: "usb" }],
      reloadFailed: ["https://a.example", "https://a.example/path", 3],
    }),
  ).toEqual({ settings: [camera], reloadFailed: ["https://a.example"] });
  expect(admitSiteSettingsUpdate(null)).toEqual({
    settings: [],
    reloadFailed: [],
  });
  expect(
    admitBrowserSiteSettings(
      Array.from({ length: MAX_SITE_SETTINGS + 10 }, () => camera),
    ),
  ).toHaveLength(MAX_SITE_SETTINGS);
});

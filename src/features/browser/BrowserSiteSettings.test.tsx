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
  browserSiteSettingsResetAll: () => resetAll(),
  browserSiteReloadTabs: () => reloadTabs(),
}));
vi.mock("../../ports/nativeBrowserPort", () => ({
  browserSiteSettingReset: (origin: string, kind: string) =>
    reset(origin, kind),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => undefined),
}));

import { BrowserSiteSettings } from "./BrowserSiteSettings";
import {
  admitBrowserSiteSettings,
  admitSiteSettingsView,
  MAX_SITE_SETTINGS,
} from "./browserSiteSettingsModel";

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
const view = (settings: unknown[], staleSites: string[] = []) => ({
  settings,
  staleSites,
});
const STALE =
  "Tabs on https://a.example have not reloaded since an Allow ended and may still use it. Reload or close them.";

beforeEach(() => {
  vi.mocked(listen).mockReset();
  vi.mocked(listen).mockResolvedValue(() => undefined);
  for (const mock of [list, set, reset, resetAll, reloadTabs]) mock.mockReset();
  list.mockResolvedValue(view([camera, location]));
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

function renderSettings() {
  render(
    <>
      <p id="note">Saved answers</p>
      <BrowserSiteSettings describedBy="note" />
    </>,
  );
}

const cameraSelect = () =>
  screen.findByRole("combobox", { name: "Camera for https://a.example" });

it("lists every saved site answer with its setting", async () => {
  renderSettings();
  const table = await screen.findByRole("table");
  expect(table).toHaveAccessibleDescription("Saved answers");
  const rows = within(table).getAllByRole("row").slice(1);
  expect(rows).toHaveLength(2);
  expect(rows[0]).toHaveTextContent("https://a.example");
  expect(rows[0]).toHaveTextContent("Camera");
  expect(await cameraSelect()).toHaveValue("allow");
  expect(
    screen.getByRole("combobox", {
      name: "Location for https://b.example:8443",
    }),
  ).toHaveValue("deny");
  expect(screen.queryByRole("alert")).toBeNull();
});

it("changes a site's answer through Rust and shows the stored list", async () => {
  set.mockResolvedValue(view([{ ...camera, decision: "deny" }, location]));
  renderSettings();
  const select = await cameraSelect();
  fireEvent.change(select, { target: { value: "deny" } });
  expect(set).toHaveBeenCalledWith("https://a.example", "camera", "deny");
  await vi.waitFor(() => expect(select).toHaveValue("deny"));
});

it("resets one answer so the site asks again", async () => {
  reset.mockResolvedValue(view([location]));
  renderSettings();
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

it.each(["Cancel", "Escape", "Confirm"])(
  "resets every answer only on Confirm, after %s",
  async (answer) => {
    // Tauri's native confirm resolves asynchronously, including on dismissal.
    vi.spyOn(window, "confirm").mockImplementation(
      () => Promise.resolve(false) as unknown as boolean,
    );
    let saved = [camera, location];
    list.mockImplementation(async () => view(saved));
    resetAll.mockImplementation(async () => {
      saved = [];
      return view(saved);
    });
    renderSettings();
    const button = await screen.findByRole("button", {
      name: "Reset all site permissions",
    });
    fireEvent.click(button);
    const dialog = await screen.findByRole("dialog", {
      name: "Reset all site permissions?",
    });
    expect(dialog).toHaveAccessibleDescription(
      "This removes every saved Allow and Block decision. Sites will ask again, and Sage will try to reload pages that were allowed access.",
    );
    expect(
      within(dialog).getByRole("button", { name: "Cancel" }),
    ).toHaveFocus();
    if (answer === "Escape") {
      fireEvent.keyDown(dialog, { key: "Escape" });
    } else {
      fireEvent.click(
        within(dialog).getByRole("button", {
          name: answer === "Confirm" ? "Reset all site permissions" : "Cancel",
        }),
      );
    }
    await vi.waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    // Remount to read the stored data, rather than trusting the current UI.
    cleanup();
    renderSettings();
    if (answer === "Confirm") {
      expect(
        await screen.findByText("No site permissions saved yet."),
      ).toBeVisible();
    } else {
      expect(await cameraSelect()).toHaveValue("allow");
      expect(
        screen.getByRole("combobox", {
          name: "Location for https://b.example:8443",
        }),
      ).toHaveValue("deny");
    }
  },
);

it("shows the sites Rust still reports stale and reloads them on request", async () => {
  const blocked = [{ ...camera, decision: "deny" }, location];
  set.mockResolvedValue(view(blocked, ["https://a.example"]));
  reloadTabs
    .mockResolvedValueOnce(view(blocked, ["https://a.example"]))
    .mockResolvedValueOnce(view(blocked));
  renderSettings();
  const select = await cameraSelect();
  fireEvent.change(select, { target: { value: "deny" } });
  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent(STALE);
  expect(select).toHaveValue("deny");
  fireEvent.click(within(alert).getByRole("button", { name: "Reload tabs" }));
  await vi.waitFor(() => expect(reloadTabs).toHaveBeenCalledOnce());
  await vi.waitFor(() =>
    expect(screen.getByRole("button", { name: "Reload tabs" })).toBeEnabled(),
  );
  expect(screen.getByRole("alert")).toHaveTextContent(STALE);
  fireEvent.click(screen.getByRole("button", { name: "Reload tabs" }));
  await vi.waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
});

it("keeps showing stale sites that a later change still reports", async () => {
  list.mockResolvedValue(view([camera, location], ["https://a.example"]));
  set.mockResolvedValue(
    view([camera, { ...location, decision: "allow" }], ["https://a.example"]),
  );
  renderSettings();
  expect(await screen.findByRole("alert")).toHaveTextContent(STALE);
  const other = screen.getByRole("combobox", {
    name: "Location for https://b.example:8443",
  });
  fireEvent.change(other, { target: { value: "allow" } });
  await vi.waitFor(() => expect(other).toHaveValue("allow"));
  expect(screen.getByRole("alert")).toHaveTextContent(STALE);
});

it("clears the stale warning when Rust reports the page reloaded", async () => {
  let deliver!: (payload: unknown) => void;
  vi.mocked(listen).mockImplementationOnce(async (_event, handler) => {
    deliver = (payload) =>
      handler({ event: "browser:site-settings", id: 1, payload });
    return () => undefined;
  });
  list.mockResolvedValue(view([camera], ["https://a.example"]));
  renderSettings();
  expect(await screen.findByRole("alert")).toHaveTextContent(STALE);
  act(() => deliver(view([camera])));
  expect(screen.queryByRole("alert")).toBeNull();
});

it("says when nothing is saved", async () => {
  list.mockResolvedValue(view([]));
  renderSettings();
  expect(
    await screen.findByText("No site permissions saved yet."),
  ).toBeVisible();
  expect(screen.queryByRole("table")).toBeNull();
});

it("keeps the list and reports a failed save", async () => {
  set.mockRejectedValue(new Error("Sage keeps at most 256 site permissions"));
  renderSettings();
  const select = await cameraSelect();
  fireEvent.change(select, { target: { value: "deny" } });
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "The site permission could not be changed.",
  );
  expect(select).toHaveValue("allow");
  expect(select).toBeEnabled();
});

it("offers Retry when loading fails", async () => {
  list.mockRejectedValueOnce(new Error("unavailable"));
  renderSettings();
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
  renderSettings();
  await screen.findByRole("table");
  await vi.waitFor(() => expect(deliver).toBeTypeOf("function"));
  act(() => deliver(view([location])));
  expect(
    screen.queryByRole("combobox", { name: "Camera for https://a.example" }),
  ).toBeNull();
  expect(
    screen.getByRole("combobox", {
      name: "Location for https://b.example:8443",
    }),
  ).toBeVisible();
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
  renderSettings();
  await vi.waitFor(() => expect(registered).toBeTypeOf("function"));
  expect(list).not.toHaveBeenCalled();
  await act(async () => registered());
  await vi.waitFor(() => expect(list).toHaveBeenCalledOnce());
  act(() => deliver(view([location])));
  await act(async () => answer(view([camera, location])));
  expect(
    screen.queryByRole("combobox", { name: "Camera for https://a.example" }),
  ).toBeNull();
  expect(
    screen.getByRole("combobox", {
      name: "Location for https://b.example:8443",
    }),
  ).toBeVisible();
});

it("reads the latest settings after an earlier event during registration", async () => {
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
  list.mockResolvedValueOnce(view([]));
  renderSettings();
  await vi.waitFor(() => expect(registered).toBeTypeOf("function"));
  act(() => deliver(view([camera])));
  await act(async () => registered());
  expect(
    await screen.findByText("No site permissions saved yet."),
  ).toBeVisible();
  expect(screen.queryByRole("table")).toBeNull();
});

it("admits only well-formed rows and sites, bounded to the store's cap", () => {
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
    admitBrowserSiteSettings(
      Array.from({ length: MAX_SITE_SETTINGS + 10 }, () => camera),
    ),
  ).toHaveLength(MAX_SITE_SETTINGS);
  expect(
    admitSiteSettingsView({
      settings: [camera, { ...camera, kind: "usb" }],
      staleSites: ["https://a.example", "https://a.example/path", 3],
    }),
  ).toEqual({ settings: [camera], staleSites: ["https://a.example"] });
  expect(admitSiteSettingsView(null)).toEqual({
    settings: [],
    staleSites: [],
  });
});

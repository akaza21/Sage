import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

const native = vi.hoisted(() => ({
  overview: vi.fn(),
  importCsv: vi.fn(),
  cancelImport: vi.fn(),
  deleteImportedFile: vi.fn(),
  update: vi.fn(),
  remove: vi.fn(),
  removeAll: vi.fn(),
  reveal: vi.fn(),
  copy: vi.fn(),
}));
const runtime = vi.hoisted(() => ({ desktop: true }));

vi.mock("../../ports/nativeInvoke", () => ({
  isTauriRuntime: () => runtime.desktop,
}));
vi.mock("../../ports/nativePasswordsPort", () => ({
  passwordsOverview: (query: string) => native.overview(query),
  passwordsImportCsv: () => native.importCsv(),
  passwordsCancelImport: () => native.cancelImport(),
  passwordsDeleteImportedFile: () => native.deleteImportedFile(),
  passwordsUpdate: (...args: unknown[]) => native.update(...args),
  passwordsDelete: (id: string) => native.remove(id),
  passwordsDeleteAll: () => native.removeAll(),
  passwordsReveal: (id: string) => native.reveal(id),
  passwordsCopy: (id: string) => native.copy(id),
}));

import { matchingSettingsEntryIds } from "../../domain/settingsNavigation";
import { SettingsPasswordsPage } from "./SettingsPasswordsPage";
import { REVEAL_MS } from "./savedPasswords";

const all = () => matchingSettingsEntryIds("", "passwords");
const ADA = { id: "a1", origin: "https://example.com", username: "ada" };
const ready = (entries = [ADA]) => ({
  status: "ready",
  total: entries.length,
  matching: entries.length,
  entries,
});

beforeEach(() => {
  vi.clearAllMocks();
  runtime.desktop = true;
  native.overview.mockResolvedValue(ready([]));
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

it("shows the empty Codex page: search, count, Refresh and no passwords", async () => {
  render(<SettingsPasswordsPage visible={all()} />);
  expect(screen.getByRole("heading", { name: "Passwords" })).toBeVisible();
  expect(
    screen.getByText(
      "Passwords you've saved for websites. Each use requires your confirmation.",
    ),
  ).toBeVisible();
  expect(
    screen.getByRole("searchbox", { name: "Search passwords" }),
  ).toBeVisible();
  expect(await screen.findByText("No saved passwords")).toBeVisible();
  expect(
    screen.getByRole("heading", { name: "Saved passwords (0)" }),
  ).toBeVisible();
  expect(screen.getByRole("button", { name: "Refresh" })).toBeEnabled();
  expect(
    screen.queryByRole("button", { name: "Delete all saved passwords" }),
  ).toBeNull();
});

it("says where saved passwords are available instead of offering controls", async () => {
  native.overview.mockResolvedValue({ status: "unsupported" });
  render(<SettingsPasswordsPage visible={all()} />);
  expect(
    await screen.findByText(/Saved passwords are available on Windows/),
  ).toBeVisible();
  expect(screen.queryByRole("button", { name: "Import CSV…" })).toBeNull();
  cleanup();
  runtime.desktop = false;
  render(<SettingsPasswordsPage visible={all()} />);
  expect(
    screen.getByText("Saved passwords are available in the Sage desktop app."),
  ).toBeVisible();
  expect(native.overview).toHaveBeenCalledTimes(1);
});

it("searches through Rust and lists sites and usernames, never passwords", async () => {
  native.overview.mockResolvedValue(ready());
  render(<SettingsPasswordsPage visible={all()} />);
  const list = await screen.findByRole("list", { name: "Saved passwords" });
  expect(within(list).getByText("example.com")).toBeVisible();
  expect(within(list).getByText("ada")).toBeVisible();
  fireEvent.change(
    screen.getByRole("searchbox", { name: "Search passwords" }),
    {
      target: { value: "exam" },
    },
  );
  await vi.waitFor(() =>
    expect(native.overview).toHaveBeenLastCalledWith("exam"),
  );
  expect(native.overview).toHaveBeenCalledTimes(2);
});

it("reveals a password only after Rust confirms, and hides it after 30 seconds", async () => {
  native.overview.mockResolvedValue(ready());
  native.reveal.mockResolvedValue("hunter2");
  render(<SettingsPasswordsPage visible={all()} />);
  const row = await screen.findByRole("group", { name: "ada on example.com" });
  vi.useFakeTimers();
  await act(async () => {
    fireEvent.click(within(row).getByRole("button", { name: "Show" }));
  });
  expect(native.reveal).toHaveBeenCalledWith("a1");
  expect(
    screen.getByLabelText("Password for ada on example.com"),
  ).toHaveTextContent("hunter2");
  act(() => vi.advanceTimersByTime(REVEAL_MS));
  expect(screen.queryByText("hunter2")).toBeNull();
});

it("keeps a password hidden when the OS confirmation fails", async () => {
  native.overview.mockResolvedValue(ready());
  native.reveal.mockRejectedValue(
    "Windows didn't confirm it's you, so the password stays hidden.",
  );
  render(<SettingsPasswordsPage visible={all()} />);
  const row = await screen.findByRole("group", { name: "ada on example.com" });
  fireEvent.click(within(row).getByRole("button", { name: "Show" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Windows didn't confirm it's you",
  );
  expect(screen.queryByLabelText("Password for ada on example.com")).toBeNull();
});

it("imports a CSV, reports counts and offers to delete the plaintext file", async () => {
  native.importCsv.mockResolvedValue({
    added: 2,
    updated: 0,
    unchanged: 1,
    skipped: 1,
    fileName: "Chrome Passwords.csv",
  });
  native.deleteImportedFile.mockResolvedValue(undefined);
  render(<SettingsPasswordsPage visible={all()} />);
  await screen.findByText("No saved passwords");
  native.overview.mockResolvedValue(ready());
  fireEvent.click(screen.getByRole("button", { name: "Import CSV…" }));
  expect(
    await screen.findByText(
      /Imported Chrome Passwords.csv: 2 added, 1 already saved, 1 skipped\./,
    ),
  ).toBeVisible();
  expect(await screen.findByText("example.com")).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Delete file" }));
  expect(
    await screen.findByText("Deleted Chrome Passwords.csv."),
  ).toBeVisible();
  expect(native.deleteImportedFile).toHaveBeenCalledTimes(1);
});

it("edits a username, and deletes one or all passwords after confirming", async () => {
  native.overview.mockResolvedValue(ready());
  native.update.mockResolvedValue(undefined);
  native.remove.mockResolvedValue(undefined);
  native.removeAll.mockResolvedValue(undefined);
  const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
  render(<SettingsPasswordsPage visible={all()} />);
  const row = await screen.findByRole("group", { name: "ada on example.com" });
  fireEvent.click(within(row).getByRole("button", { name: "Edit" }));
  const form = screen.getByRole("form", { name: "Edit ada on example.com" });
  fireEvent.change(within(form).getByLabelText("Username"), {
    target: { value: "ada@example.com" },
  });
  fireEvent.click(within(form).getByRole("button", { name: "Save" }));
  await vi.waitFor(() =>
    expect(native.update).toHaveBeenCalledWith("a1", "ada@example.com", null),
  );
  await vi.waitFor(() =>
    expect(screen.queryByRole("form", { name: /Edit/ })).toBeNull(),
  );
  fireEvent.click(
    within(screen.getByRole("group", { name: "ada on example.com" })).getByRole(
      "button",
      { name: "Delete" },
    ),
  );
  expect(confirm).toHaveBeenLastCalledWith("Delete ada on example.com?");
  await vi.waitFor(() => expect(native.remove).toHaveBeenCalledWith("a1"));
  fireEvent.click(
    await screen.findByRole("button", { name: "Delete all saved passwords" }),
  );
  expect(confirm).toHaveBeenLastCalledWith(
    "Delete all 1 saved passwords? This can't be undone.",
  );
  await vi.waitFor(() => expect(native.removeAll).toHaveBeenCalledTimes(1));
});

it("cancels a running import and says nothing was saved", async () => {
  let finish: (value: null) => void = () => {};
  native.importCsv.mockReturnValue(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  native.cancelImport.mockImplementation(async () => finish(null));
  render(<SettingsPasswordsPage visible={all()} />);
  await screen.findByText("No saved passwords");
  fireEvent.click(screen.getByRole("button", { name: "Import CSV…" }));
  fireEvent.click(await screen.findByRole("button", { name: "Cancel import" }));
  expect(
    await screen.findByText("Import cancelled. Nothing was saved."),
  ).toBeVisible();
  expect(native.cancelImport).toHaveBeenCalledTimes(1);
  expect(screen.getByRole("button", { name: "Import CSV…" })).toBeEnabled();
});

it("cancels a running import when the page closes", async () => {
  native.importCsv.mockReturnValue(new Promise(() => {}));
  const { unmount } = render(<SettingsPasswordsPage visible={all()} />);
  await screen.findByText("No saved passwords");
  fireEvent.click(screen.getByRole("button", { name: "Import CSV…" }));
  await screen.findByRole("button", { name: "Cancel import" });
  unmount();
  expect(native.cancelImport).toHaveBeenCalledTimes(1);
});

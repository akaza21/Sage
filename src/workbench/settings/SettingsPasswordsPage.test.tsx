import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { Activity } from "react";
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
import {
  REVEAL_MS,
  SEARCH_DEBOUNCE_MS,
  type PasswordImportReport,
} from "./savedPasswords";

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

it("says clipboard cleanup is an attempt when a password is copied", async () => {
  native.overview.mockResolvedValue(ready());
  native.copy.mockResolvedValue(undefined);
  render(<SettingsPasswordsPage visible={all()} />);
  const row = await screen.findByRole("group", { name: "ada on example.com" });
  fireEvent.click(within(row).getByRole("button", { name: "Copy" }));
  expect(
    await screen.findByText(
      "Password copied. Sage will try to clear it from the clipboard in 30 seconds.",
    ),
  ).toBeVisible();
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

it("hides a revealed password once an import may have replaced it", async () => {
  native.overview.mockResolvedValue(ready());
  native.reveal.mockResolvedValue("old-secret");
  native.importCsv.mockResolvedValue({
    added: 0,
    updated: 1,
    unchanged: 0,
    skipped: 0,
    fileName: "passwords.csv",
  });
  render(<SettingsPasswordsPage visible={all()} />);
  const row = await screen.findByRole("group", { name: "ada on example.com" });
  await act(async () => {
    fireEvent.click(within(row).getByRole("button", { name: "Show" }));
  });
  expect(
    screen.getByLabelText("Password for ada on example.com"),
  ).toHaveTextContent("old-secret");
  vi.useFakeTimers();
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Import CSV…" }));
  });
  expect(
    screen.getByText(/Imported passwords.csv: 0 added, 1 updated\./),
  ).toBeVisible();
  expect(screen.queryByText("old-secret")).toBeNull();
  // Its hide timer went with it: a new reveal is not cut short by the old one.
  native.reveal.mockResolvedValue("new-secret");
  await act(async () => {
    fireEvent.click(within(row).getByRole("button", { name: "Show" }));
  });
  act(() => vi.advanceTimersByTime(REVEAL_MS - 1));
  expect(
    screen.getByLabelText("Password for ada on example.com"),
  ).toHaveTextContent("new-secret");
});

it("hides a revealed password when Refresh may bring a newer one from another window", async () => {
  native.overview.mockResolvedValue(ready());
  native.reveal.mockResolvedValue("old-secret");
  render(<SettingsPasswordsPage visible={all()} />);
  const row = await screen.findByRole("group", { name: "ada on example.com" });
  await act(async () => {
    fireEvent.click(within(row).getByRole("button", { name: "Show" }));
  });
  expect(screen.getByText("old-secret")).toBeVisible();
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
  });
  expect(screen.queryByText("old-secret")).toBeNull();
  expect(native.overview).toHaveBeenCalledTimes(2);
});

it("drops a reveal the OS confirmed after the vault was reloaded", async () => {
  native.overview.mockResolvedValue(ready());
  let confirm: (password: string) => void = () => {};
  native.reveal.mockReturnValue(
    new Promise<string>((resolve) => {
      confirm = resolve;
    }),
  );
  render(<SettingsPasswordsPage visible={all()} />);
  const row = await screen.findByRole("group", { name: "ada on example.com" });
  fireEvent.click(within(row).getByRole("button", { name: "Show" }));
  // Another window's change arrives while the OS prompt is open.
  fireEvent.change(
    screen.getByRole("searchbox", { name: "Search passwords" }),
    { target: { value: "ada" } },
  );
  await vi.waitFor(() =>
    expect(native.overview).toHaveBeenLastCalledWith("ada"),
  );
  await act(async () => confirm("old-secret"));
  expect(screen.queryByText("old-secret")).toBeNull();
});

function Suspendable({ mode }: { mode: "visible" | "hidden" }) {
  return (
    <Activity mode={mode}>
      <SettingsPasswordsPage visible={all()} />
    </Activity>
  );
}

it("forgets revealed passwords and a reveal still at the OS prompt while Settings is suspended", async () => {
  native.overview.mockResolvedValue(ready());
  native.reveal.mockResolvedValueOnce("shown-secret");
  const view = render(<Suspendable mode="visible" />);
  const row = await screen.findByRole("group", { name: "ada on example.com" });
  await act(async () => {
    fireEvent.click(within(row).getByRole("button", { name: "Show" }));
  });
  expect(screen.getByText("shown-secret")).toBeInTheDocument();
  await act(async () => view.rerender(<Suspendable mode="hidden" />));
  expect(screen.queryByText("shown-secret")).toBeNull();
  await act(async () => view.rerender(<Suspendable mode="visible" />));
  expect(screen.queryByText("shown-secret")).toBeNull();

  let confirm: (password: string) => void = () => {};
  native.reveal.mockReturnValueOnce(
    new Promise<string>((resolve) => {
      confirm = resolve;
    }),
  );
  fireEvent.click(
    within(
      await screen.findByRole("group", { name: "ada on example.com" }),
    ).getByRole("button", { name: "Show" }),
  );
  await act(async () => view.rerender(<Suspendable mode="hidden" />));
  await act(async () => confirm("late-secret"));
  await act(async () => view.rerender(<Suspendable mode="visible" />));
  expect(screen.queryByText("late-secret")).toBeNull();
});

it("shows an import that finished while Settings was suspended, with Import available again", async () => {
  let finish: (report: unknown) => void = () => {};
  native.importCsv.mockReturnValue(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  native.cancelImport.mockResolvedValue(false);
  const view = render(<Suspendable mode="visible" />);
  await screen.findByText("No saved passwords");
  fireEvent.click(screen.getByRole("button", { name: "Import CSV…" }));
  expect(
    await screen.findByRole("button", { name: "Cancel import" }),
  ).toBeVisible();
  await act(async () => view.rerender(<Suspendable mode="hidden" />));
  // Hiding asks Rust to cancel, but the commit had already won.
  expect(native.cancelImport).toHaveBeenCalledTimes(1);
  native.overview.mockResolvedValue(ready());
  await act(async () =>
    finish({
      added: 1,
      updated: 0,
      unchanged: 0,
      skipped: 0,
      fileName: "passwords.csv",
    }),
  );
  await act(async () => view.rerender(<Suspendable mode="visible" />));
  expect(
    await screen.findByText(/Imported passwords.csv: 1 added\./),
  ).toBeVisible();
  expect(screen.queryByRole("button", { name: "Cancel import" })).toBeNull();
  expect(screen.getByRole("button", { name: "Import CSV…" })).toBeEnabled();
});

it("clears a new password typed into Edit while Settings is suspended", async () => {
  native.overview.mockResolvedValue(ready());
  const view = render(<Suspendable mode="visible" />);
  const row = await screen.findByRole("group", { name: "ada on example.com" });
  fireEvent.click(within(row).getByRole("button", { name: "Edit" }));
  const form = screen.getByRole("form", { name: "Edit ada on example.com" });
  fireEvent.change(within(form).getByLabelText("Username"), {
    target: { value: "ada@example.com" },
  });
  const password = within(form).getByLabelText("New password");
  fireEvent.change(password, { target: { value: "draft-secret" } });
  await act(async () => view.rerender(<Suspendable mode="hidden" />));
  // The hidden tree keeps its DOM, so the draft must be gone from it too.
  expect(password).toHaveValue("");
  await act(async () => view.rerender(<Suspendable mode="visible" />));
  expect(within(form).getByLabelText("New password")).toHaveValue("");
  expect(within(form).getByLabelText("Username")).toHaveValue(
    "ada@example.com",
  );
});

it("keeps a password revealed just after the page opens", async () => {
  native.overview.mockResolvedValue(ready());
  native.reveal.mockResolvedValueOnce("early-secret");
  render(<SettingsPasswordsPage visible={all()} />);
  const row = await screen.findByRole("group", { name: "ada on example.com" });
  await act(async () => {
    fireEvent.click(within(row).getByRole("button", { name: "Show" }));
  });
  await act(
    () => new Promise((resolve) => setTimeout(resolve, SEARCH_DEBOUNCE_MS * 2)),
  );
  expect(screen.getByText("early-secret")).toBeVisible();
  expect(native.overview).toHaveBeenCalledTimes(1);
});

it("edits a username", async () => {
  native.overview.mockResolvedValue(ready());
  native.update.mockResolvedValue(undefined);
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
});

it.each(["Delete", "Delete all saved passwords"])(
  "%s preserves saved entries on Cancel and Escape, and deletes only on confirmation",
  async (action) => {
    // Model the native vault at the IPC boundary, including reloads after dismissal.
    let entries = [
      { id: "one", origin: "https://one.example.test", username: "user-one" },
      { id: "two", origin: "https://two.example.test", username: "user-two" },
    ];
    native.overview.mockImplementation(async () => ready(entries));
    native.remove.mockImplementation(async (id: string) => {
      entries = entries.filter((entry) => entry.id !== id);
    });
    native.removeAll.mockImplementation(async () => {
      entries = [];
    });
    // Reproduce Tauri's asynchronous replacement of the browser global.
    vi.spyOn(window, "confirm").mockImplementation((() =>
      Promise.resolve(false)) as unknown as typeof window.confirm);
    render(<SettingsPasswordsPage visible={all()} />);
    const open = async () => {
      const row = await screen.findByRole("group", {
        name: "user-one on one.example.test",
      });
      const trigger = action === "Delete" ? within(row) : screen;
      trigger.getByRole("button", { name: action }).focus();
      fireEvent.click(trigger.getByRole("button", { name: action }));
      return screen.getByRole("dialog", {
        name:
          action === "Delete"
            ? "Delete user-one on one.example.test?"
            : "Delete all 2 saved passwords?",
      });
    };

    for (const dismissal of ["Cancel", "Escape"]) {
      const dialog = await open();
      expect(dialog).toHaveAccessibleDescription(/can't be undone/);
      expect(
        within(dialog).getByRole("button", { name: "Cancel" }),
      ).toHaveFocus();
      if (dismissal === "Cancel") {
        fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
      } else {
        fireEvent.keyDown(dialog, { key: "Escape" });
      }
      expect(screen.queryByRole("dialog")).toBeNull();
      fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
      expect(await screen.findByText("Saved passwords (2)")).toBeVisible();
      expect(
        screen.getByRole("group", { name: "user-one on one.example.test" }),
      ).toBeVisible();
      expect(
        screen.getByRole("group", { name: "user-two on two.example.test" }),
      ).toBeVisible();
      expect(entries).toHaveLength(2);
    }

    const dialog = await open();
    fireEvent.click(
      within(dialog).getByRole("button", {
        name:
          action === "Delete"
            ? "Delete password"
            : "Delete all saved passwords",
      }),
    );
    expect(
      await screen.findByText(
        action === "Delete" ? "Saved passwords (1)" : "No saved passwords",
      ),
    ).toBeVisible();
    expect(
      screen.queryByRole("group", { name: "user-one on one.example.test" }),
    ).toBeNull();
    if (action === "Delete") {
      expect(
        screen.getByRole("group", { name: "user-two on two.example.test" }),
      ).toBeVisible();
    }
  },
);

it.each(["Cancel import", "close page"])(
  "%s prevents a late import from saving or showing passwords, including after reopening",
  async (action) => {
    // Model the native import at IPC: completion attempts to persist, unless
    // cancellation reached it. The Rust suite checks the real vault rollback.
    let entries: (typeof ADA)[] = [];
    let cancelled = false;
    let finish: () => void = () => {};
    native.overview.mockImplementation(async () => ready(entries));
    native.importCsv.mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = () => {
            if (!cancelled) entries = [ADA];
            resolve(
              cancelled
                ? null
                : {
                    added: 1,
                    updated: 0,
                    unchanged: 0,
                    skipped: 0,
                    fileName: "passwords.csv",
                  },
            );
          };
        }),
    );
    native.cancelImport.mockImplementation(async () => {
      cancelled = true;
      return true;
    });
    const { unmount } = render(<SettingsPasswordsPage visible={all()} />);
    await screen.findByText("No saved passwords");
    fireEvent.click(screen.getByRole("button", { name: "Import CSV…" }));
    const cancel = await screen.findByRole("button", { name: "Cancel import" });
    if (action === "close page") unmount();
    else fireEvent.click(cancel);

    await act(async () => finish());
    if (action === "Cancel import") {
      expect(
        screen.getByText("Import cancelled. Nothing was saved."),
      ).toBeVisible();
      expect(screen.queryByText(/Imported passwords.csv/)).toBeNull();
      expect(
        screen.queryByRole("list", { name: "Saved passwords" }),
      ).toBeNull();
      expect(screen.getByRole("button", { name: "Import CSV…" })).toBeEnabled();
      unmount();
    }
    render(<SettingsPasswordsPage visible={all()} />);
    expect(await screen.findByText("No saved passwords")).toBeVisible();
    expect(
      screen.getByRole("heading", { name: "Saved passwords (0)" }),
    ).toBeVisible();
    expect(screen.queryByText("example.com")).toBeNull();
    expect(entries).toEqual([]);
  },
);

it("reports too-late cancellation before displaying a delayed committed import", async () => {
  let entries: (typeof ADA)[] = [];
  let finish: () => void = () => {};
  native.overview.mockImplementation(async () => ready(entries));
  native.importCsv.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = () =>
          resolve({
            added: 1,
            updated: 0,
            unchanged: 0,
            skipped: 0,
            fileName: "passwords.csv",
          });
      }),
  );
  native.cancelImport.mockResolvedValue(false);
  render(<SettingsPasswordsPage visible={all()} />);
  await screen.findByText("No saved passwords");
  fireEvent.click(screen.getByRole("button", { name: "Import CSV…" }));
  const cancel = await screen.findByRole("button", { name: "Cancel import" });
  // Native commit has finished, but its IPC response has not arrived yet.
  entries = [ADA];
  fireEvent.click(cancel);
  expect(
    await screen.findByText(
      "The import can no longer be cancelled. Its results will appear when it finishes.",
    ),
  ).toBeVisible();
  await act(async () => finish());
  expect(
    await screen.findByText(
      "Imported passwords.csv: 1 added. The file still holds these passwords in plain text.",
    ),
  ).toBeVisible();
  expect(await screen.findByText("example.com")).toBeVisible();
  expect(screen.queryByText("Import cancelled. Nothing was saved.")).toBeNull();
  expect(
    screen.queryByText(/Its results will appear when it finishes/),
  ).toBeNull();
});

it.each([
  ["completed import", false],
  ["next import", false],
  ["completed import", true],
  ["next import", true],
  ["completion response", false],
  ["completion response", true],
])(
  "ignores a late cancel response during the %s (rejection: %s)",
  async (phase, rejection) => {
    let finishImport: (report: PasswordImportReport) => void = () => {};
    let answerCancel: () => void = () => {};
    native.importCsv.mockImplementation(
      () => new Promise((resolve) => (finishImport = resolve)),
    );
    native.cancelImport.mockImplementation(
      () =>
        new Promise((resolve, reject) => {
          answerCancel = () =>
            rejection ? reject("Cancellation unavailable.") : resolve(false);
        }),
    );
    render(<SettingsPasswordsPage visible={all()} />);
    await screen.findByText("No saved passwords");
    fireEvent.click(screen.getByRole("button", { name: "Import CSV…" }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel import" }));
    const report = {
      added: 1,
      updated: 0,
      unchanged: 0,
      skipped: 0,
      fileName: "first.csv",
    };
    await act(async () => {
      finishImport(report);
      if (phase === "completion response") {
        await Promise.resolve();
        answerCancel();
      }
    });
    expect(
      await screen.findByText(/Imported first.csv: 1 added/),
    ).toBeVisible();

    if (phase === "next import") {
      fireEvent.click(screen.getByRole("button", { name: "Import CSV…" }));
      expect(
        screen.getByRole("button", { name: "Cancel import" }),
      ).toBeVisible();
    }
    if (phase !== "completion response") await act(async () => answerCancel());
    expect(
      screen.queryByText(/Its results will appear when it finishes/),
    ).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();

    if (phase === "next import") {
      await act(async () =>
        finishImport({ ...report, fileName: "second.csv" }),
      );
      expect(
        await screen.findByText(/Imported second.csv: 1 added/),
      ).toBeVisible();
    }
    expect(screen.getByRole("button", { name: "Import CSV…" })).toBeEnabled();
  },
);

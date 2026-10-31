import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

const port = vi.hoisted(() => ({
  get: vi.fn(),
  choose: vi.fn(),
  reset: vi.fn(),
  emit: null as ((payload: unknown) => void) | null,
  unlisten: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: async (
    _name: string,
    handler: (event: { payload: unknown }) => void,
  ) => {
    port.emit = (payload) => handler({ payload });
    return port.unlisten;
  },
}));

vi.mock("../../ports/nativeBrowserDownloadFolderPort", async (actual) => ({
  ...(await actual<Record<string, unknown>>()),
  browserDownloadFolder: () => port.get(),
  browserChooseDownloadFolder: () => port.choose(),
  browserResetDownloadFolder: () => port.reset(),
}));

import { admitDownloadFolder } from "../../ports/nativeBrowserDownloadFolderPort";
import { BrowserDownloadFolder } from "./BrowserDownloadFolder";

const FOLDER = "C:\\Users\\me\\Downloads\\Sage";

beforeEach(() => {
  vi.clearAllMocks();
  port.emit = null;
  port.get.mockResolvedValue({ path: null, available: false });
  port.reset.mockResolvedValue({ path: null, available: false });
});

afterEach(cleanup);

const change = () => screen.getByRole("button", { name: "Change…" });
const reset = () => screen.getByRole("button", { name: "Reset" });

it("says the first download asks while no folder is saved", async () => {
  render(<BrowserDownloadFolder />);
  expect(
    await screen.findByText(/Not set\. The next download asks where to save/),
  ).toBeVisible();
  await waitFor(() => expect(change()).toBeEnabled());
  expect(reset()).toBeDisabled();
  expect(screen.getByText(/Programs, scripts, and shortcuts/)).toBeVisible();
});

it("changes the folder through the native picker and resets it", async () => {
  port.choose.mockResolvedValue({ path: FOLDER, available: true });
  render(<BrowserDownloadFolder />);
  await waitFor(() => expect(change()).toBeEnabled());
  fireEvent.click(change());
  expect(port.choose).toHaveBeenCalledTimes(1);
  expect(port.choose).toHaveBeenCalledWith();
  expect(await screen.findByText(FOLDER)).toHaveAttribute("title", FOLDER);
  expect(reset()).toBeEnabled();
  fireEvent.click(reset());
  expect(
    await screen.findByText(/Not set\. The next download asks/),
  ).toBeVisible();
  expect(port.reset).toHaveBeenCalledTimes(1);
  expect(reset()).toBeDisabled();
});

it("shows a saved folder that went missing and that the next download asks", async () => {
  port.get.mockResolvedValue({ path: FOLDER, available: false });
  render(<BrowserDownloadFolder />);
  expect(
    await screen.findByText(
      `${FOLDER} is missing or read-only. The next download asks where to save.`,
    ),
  ).toBeVisible();
  expect(reset()).toBeEnabled();
});

it("keeps the old folder and reports a refused change", async () => {
  port.get.mockResolvedValue({ path: FOLDER, available: true });
  port.choose.mockRejectedValue(
    "That folder can't be used for downloads. Choose a folder you can write to.",
  );
  render(<BrowserDownloadFolder />);
  await screen.findByText(FOLDER);
  fireEvent.click(change());
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "That folder can't be used for downloads.",
  );
  expect(screen.getByText(FOLDER)).toBeVisible();
});

it("reports a failed load and still lets the user choose a folder", async () => {
  port.get.mockRejectedValue(new Error("unavailable"));
  render(<BrowserDownloadFolder />);
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "The download folder could not be loaded.",
  );
  await waitFor(() => expect(change()).toBeEnabled());
});

it("admits only the fields Rust sends", () => {
  expect(admitDownloadFolder({ path: FOLDER, available: true, x: 1 })).toEqual({
    path: FOLDER,
    available: true,
  });
  expect(admitDownloadFolder({ path: "", available: true })).toEqual({
    path: null,
    available: false,
  });
  expect(admitDownloadFolder(null)).toEqual({ path: null, available: false });
  expect(admitDownloadFolder({ path: FOLDER, available: "yes" })).toEqual({
    path: FOLDER,
    available: false,
  });
});

it("shows a folder the first download saved while Settings is open", async () => {
  const { unmount } = render(<BrowserDownloadFolder />);
  await screen.findByText(/Not set\. The next download asks/);
  await waitFor(() => expect(port.emit).not.toBeNull());
  act(() => port.emit?.({ path: FOLDER, available: true }));
  expect(await screen.findByText(FOLDER)).toBeVisible();
  expect(reset()).toBeEnabled();
  unmount();
  expect(port.unlisten).toHaveBeenCalledTimes(1);
});

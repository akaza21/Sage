import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";

import { BrowserSitePermissions } from "./BrowserSitePermissions";

const status = vi.fn();
const resolve = vi.fn();
const revoke = vi.fn();

vi.mock("../../ports/nativeInvoke", () => ({ isTauriRuntime: () => true }));
vi.mock("../../ports/nativeBrowserPort", () => ({
  browserPermissionStatus: (tabId: string) => status(tabId),
  browserPermissionResolve: (tabId: string, id: number, allow: boolean) =>
    resolve(tabId, id, allow),
  browserPermissionRevoke: (tabId: string, origin: string, kind: string) =>
    revoke(tabId, origin, kind),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => undefined),
}));

beforeEach(() => {
  status.mockReset();
  resolve.mockReset();
  revoke.mockReset();
  status.mockResolvedValue({
    tabId: "browser-1",
    origin: "https://a.example",
    pending: [
      {
        id: 7,
        tabId: "browser-1",
        origin: "https://a.example",
        kind: "camera",
      },
    ],
    decisions: [],
  });
  resolve.mockResolvedValue(undefined);
  revoke.mockResolvedValue(undefined);
});

it("shows a site permission prompt in browser chrome and resolves Allow", async () => {
  render(<BrowserSitePermissions tabId="browser-1" />);
  expect(
    await screen.findByText(/a.example wants to use your camera/i),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Allow camera" }));
  await waitFor(() =>
    expect(resolve).toHaveBeenCalledWith("browser-1", 7, true),
  );
});

it("blocks a pending permission without granting it", async () => {
  render(<BrowserSitePermissions tabId="browser-1" />);
  fireEvent.click(await screen.findByRole("button", { name: "Block camera" }));
  await waitFor(() =>
    expect(resolve).toHaveBeenCalledWith("browser-1", 7, false),
  );
});

it("shows session decisions and revokes a site's camera access", async () => {
  status.mockResolvedValueOnce({
    tabId: "browser-1",
    origin: "https://a.example",
    pending: [],
    decisions: [
      { origin: "https://a.example", kind: "camera", decision: "allow" },
    ],
  });
  render(<BrowserSitePermissions tabId="browser-1" />);
  fireEvent.click(
    await screen.findByRole("button", { name: "Site permissions" }),
  );
  expect(
    screen.getByText(/https:\/\/a.example.*camera.*allowed/i),
  ).toBeInTheDocument();
  fireEvent.click(
    screen.getByRole("button", { name: "Revoke camera for https://a.example" }),
  );
  await waitFor(() =>
    expect(revoke).toHaveBeenCalledWith(
      "browser-1",
      "https://a.example",
      "camera",
    ),
  );
});

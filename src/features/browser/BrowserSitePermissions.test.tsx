import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { Activity } from "react";
import { listen } from "@tauri-apps/api/event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { BrowserSitePermissions } from "./BrowserSitePermissions";

const status = vi.fn();
const resolve = vi.fn();
const revoke = vi.fn();

vi.mock("../../ports/nativeInvoke", () => ({ isTauriRuntime: () => true }));
vi.mock("../../ports/nativeBrowserPort", () => ({
  browserPermissionStatus: (tabId: string) => status(tabId),
  browserPermissionResolve: (tabId: string, id: number, allow: boolean) =>
    resolve(tabId, id, allow),
  browserSiteSettingReset: (origin: string, kind: string) =>
    revoke(origin, kind),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => undefined),
}));

beforeEach(() => {
  vi.mocked(listen).mockReset();
  vi.mocked(listen).mockResolvedValue(() => undefined);
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

afterEach(cleanup);

it("disposes a permission subscription that finishes registering after the drawer closes", async () => {
  let registered!: (dispose: () => void) => void;
  const dispose = vi.fn();
  vi.mocked(listen).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        registered = resolve;
      }),
  );
  const drawer = render(<BrowserSitePermissions tabId="browser-1" showEmpty />);
  await waitFor(() => expect(listen).toHaveBeenCalled());
  drawer.unmount();
  await act(async () => registered(dispose));
  expect(dispose).toHaveBeenCalledOnce();
  expect(status).not.toHaveBeenCalled();
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

it("disposes late permission registration after Activity sleeps without reading hidden state", async () => {
  let finish: ((dispose: () => void) => void) | undefined;
  const dispose = vi.fn();
  vi.mocked(listen).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const view = render(
    <Activity mode="visible">
      <BrowserSitePermissions tabId="browser-1" />
    </Activity>,
  );
  await waitFor(() => expect(finish).toBeDefined());
  view.rerender(
    <Activity mode="hidden">
      <BrowserSitePermissions tabId="browser-1" />
    </Activity>,
  );
  await act(async () => finish?.(dispose));
  expect(dispose).toHaveBeenCalledOnce();
  expect(status).not.toHaveBeenCalled();
  view.unmount();
});

it("shows the saved site decisions and resets a site's camera access", async () => {
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
    expect(revoke).toHaveBeenCalledWith("https://a.example", "camera"),
  );
});

it("warns while the page may still use access that was revoked", async () => {
  status.mockResolvedValueOnce({
    tabId: "browser-1",
    origin: "https://a.example",
    pending: [],
    decisions: [],
    staleAccess: true,
  });
  render(<BrowserSitePermissions tabId="browser-1" />);
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Reload this page to end revoked access.",
  );
});

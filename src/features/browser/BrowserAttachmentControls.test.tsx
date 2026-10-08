import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { BrowserAttachmentControls } from "./BrowserAttachmentControls";

const { attach, detach, status } = vi.hoisted(() => ({
  attach: vi.fn(async () => ({
    tabId: "browser-7",
    url: "https://example.com",
  })),
  detach: vi.fn(async () => undefined),
  status: vi.fn(async () => null as { tabId: string; url: string } | null),
}));
vi.mock("../../ports/nativeBrowserPort", () => ({
  attachBrowserTab: attach,
  detachBrowserTab: detach,
  browserAttachmentStatus: status,
}));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

it("does not let a stale initial status erase a successful attachment", async () => {
  let resolveStatus!: (value: { tabId: string; url: string } | null) => void;
  status.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        resolveStatus = resolve;
      }),
  );
  render(
    <BrowserAttachmentControls
      projectId="project-1"
      tabId="browser-7"
      ready
      chats={[{ id: "chat-1", title: "First" }]}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Attach tab to chat" }));
  await screen.findByRole("button", { name: "Detach tab from chat" });
  await act(async () => resolveStatus(null));
  expect(
    screen.getByRole("button", { name: "Detach tab from chat" }),
  ).toBeTruthy();
});

it("only attaches a settled tab to the chat the user chose and can detach it", async () => {
  render(
    <BrowserAttachmentControls
      projectId="project-1"
      tabId="browser-7"
      ready
      chats={[
        { id: "chat-1", title: "First" },
        { id: "chat-2", title: "Second" },
      ]}
    />,
  );
  fireEvent.change(
    screen.getByRole("combobox", { name: "Chat for browser tab" }),
    { target: { value: "chat-2" } },
  );
  fireEvent.click(screen.getByRole("button", { name: "Attach tab to chat" }));
  await waitFor(() =>
    expect(attach).toHaveBeenCalledWith("project-1", "chat-2", "browser-7"),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "Detach tab from chat" }),
  );
  await waitFor(() =>
    expect(detach).toHaveBeenCalledWith("project-1", "chat-2"),
  );
});

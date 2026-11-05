import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { BrowserOperationProjection } from "../../domain/browserOperation";
import type { BrowserAgentControl } from "../../ports/nativeBrowserPort";
import { BrowserToolbarPageActions } from "./BrowserAttachmentControls";

const native = vi.hoisted(() => ({
  control: null as BrowserAgentControl | null,
  listeners: new Set<(event: { payload: { tabId: string } }) => void>(),
  registration: null as Promise<void> | null,
  read: null as Promise<BrowserAgentControl | null> | null,
}));
vi.mock("../../ports/desktopRuntime", () => ({ isTauriRuntime: () => true }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (
    _name: string,
    handler: (event: { payload: { tabId: string } }) => void,
  ) => {
    await native.registration;
    native.listeners.add(handler);
    return () => native.listeners.delete(handler);
  },
}));
vi.mock("../../ports/nativeBrowserPort", () => ({
  browserAgentControl: async () => native.read ?? native.control,
  browserAttachmentStatus: async () => null,
}));

afterEach(() => {
  cleanup();
  native.control = null;
  native.listeners.clear();
  native.registration = null;
  native.read = null;
});

const approved: BrowserAgentControl = {
  projectId: "project-1",
  chatId: "chat-1",
  userControl: false,
  developerMode: true,
};

function tree(
  visible: boolean,
  revision = "1",
  operation: BrowserOperationProjection | null = null,
) {
  return (
    <BrowserToolbarPageActions
      visible={visible}
      projectId="project-1"
      tabId="browser-1"
      ready
      pageRevision={revision}
      chats={[{ id: "chat-1", title: "Research chat" }]}
      operation={operation}
      attachmentEpoch={0}
      onStopped={() => undefined}
      onError={() => undefined}
    />
  );
}

it("shows a new developer grant and its revocation while the first elevated command is pending", async () => {
  native.control = { ...approved, developerMode: false };
  const evaluation: BrowserOperationProjection = {
    projectId: "project-1",
    chatId: "chat-1",
    sessionId: "session-1",
    turnId: "turn-1",
    tabId: "browser-1",
    operationId: "evaluation-1",
    operation: "cdp",
    phase: "started",
    origin: "https://example.com",
    summary: "Runtime.evaluate",
    createdAt: 1,
    updatedAt: 1,
  };
  const view = render(tree(true, "1", evaluation));
  await screen.findByRole("button", { name: "Take over" });
  await waitFor(() => expect(native.listeners.size).toBe(1));
  expect(screen.queryByText("Developer mode")).toBeNull();

  let finish!: () => void;
  const command = new Promise<void>((resolve) => {
    finish = resolve;
  }).then(() => {
    view.rerender(
      tree(true, "1", { ...evaluation, phase: "completed", updatedAt: 2 }),
    );
  });
  try {
    native.control = approved;
    await act(async () => {
      for (const listener of native.listeners)
        listener({ payload: { tabId: "browser-1" } });
    });
    expect(screen.getByText("Developer mode")).toBeVisible();
    expect(screen.getByRole("status")).toHaveAttribute(
      "title",
      "Agent has a browser action pending · Research chat",
    );

    native.control = { ...approved, developerMode: false };
    await act(async () => {
      for (const listener of native.listeners)
        listener({ payload: { tabId: "browser-1" } });
    });
    expect(screen.queryByText("Developer mode")).toBeNull();
    expect(screen.getByRole("status")).toHaveAttribute(
      "title",
      "Agent has a browser action pending · Research chat",
    );
  } finally {
    await act(async () => {
      finish();
      await command;
    });
  }
  expect(screen.getByRole("status")).toHaveAttribute(
    "title",
    "Agent attached · Research chat",
  );
});

it("refreshes approval revocation while visible and releases its subscription while hidden", async () => {
  native.control = approved;
  const view = render(tree(true));
  await screen.findByText("Developer mode");
  await waitFor(() => expect(native.listeners.size).toBe(1));
  native.control = { ...approved, developerMode: false };
  await act(async () => {
    for (const listener of native.listeners)
      listener({ payload: { tabId: "browser-1" } });
  });
  expect(screen.queryByText("Developer mode")).toBeNull();
  expect(screen.getByRole("status")).toHaveAttribute(
    "title",
    "Agent attached · Research chat",
  );
  view.rerender(tree(false));
  expect(view.container).toBeEmptyDOMElement();
  expect(native.listeners.size).toBe(0);
  native.control = approved;
  view.rerender(tree(true));
  await screen.findByText("Developer mode");
  await waitFor(() => expect(native.listeners.size).toBe(1));
  view.unmount();
  expect(native.listeners.size).toBe(0);
});

it("releases a subscription that finishes registering after the controls sleep", async () => {
  let finish!: () => void;
  native.registration = new Promise<void>((resolve) => {
    finish = resolve;
  });
  const view = render(tree(true));
  await act(async () => undefined);
  view.rerender(tree(false));
  await act(async () => finish());
  expect(native.listeners.size).toBe(0);
  expect(view.container).toBeEmptyDOMElement();
});

it("ignores an approval read completed during sleep and refreshes from Rust on wake", async () => {
  let finish!: (control: BrowserAgentControl | null) => void;
  native.read = new Promise((resolve) => {
    finish = resolve;
  });
  const view = render(tree(true));
  view.rerender(tree(false));
  await act(async () => finish(approved));
  expect(view.container).toBeEmptyDOMElement();
  native.read = new Promise(() => undefined);
  view.rerender(tree(true, "2"));
  expect(screen.queryByText("Developer mode")).toBeNull();
  expect(screen.queryByRole("button", { name: "Stop agent" })).toBeNull();
});

import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../../app/store";
import { pinAgentChatOwner } from "../../app/agentChatOwner";
import { BrowserToolbarPageActions } from "./BrowserAttachmentControls";

const native = vi.hoisted(() => ({
  reads: [] as string[],
  attached: null as string | null,
  capture: null as Promise<unknown> | null,
}));
vi.mock("../../ports/nativeBrowserPort", () => ({
  browserStatus: async () => ({
    tabId: "browser-1",
    url: "https://example.com/",
    revision: "1",
    loading: false,
  }),
  browserPageContext: async () => native.capture,
  browserAgentControl: async (tabId: string) => {
    native.reads.push(tabId);
    return null;
  },
  browserAttachmentStatus: async () =>
    native.attached ? { tabId: "browser-1" } : null,
  attachBrowserTab: async (_project: string, chat: string) => {
    native.attached = chat;
  },
  detachBrowserTab: async () => {
    native.attached = null;
  },
}));
const initial = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  native.reads = [];
  native.attached = null;
  native.capture = null;
  pinAgentChatOwner(null);
  useWorkbenchStore.setState(initial, true);
});

it("unmounts hidden page controls and makes no agent-control reads on revisions, then resumes the chosen chat", async () => {
  const tree = (visible: boolean, revision: string) => (
    <BrowserToolbarPageActions
      visible={visible}
      projectId="project-1"
      tabId="browser-1"
      ready
      pageRevision={revision}
      chats={[
        { id: "chat-1", title: "First" },
        { id: "chat-2", title: "Second" },
      ]}
      operation={null}
      attachmentEpoch={0}
      onStopped={() => undefined}
      onError={() => undefined}
    />
  );
  const view = render(tree(true, "1"));
  fireEvent.change(
    screen.getByRole("combobox", { name: "Chat for browser tab" }),
    { target: { value: "chat-2" } },
  );
  fireEvent.click(screen.getByRole("button", { name: "Attach tab to chat" }));
  await screen.findByRole("button", { name: "Detach tab from chat" });
  view.rerender(tree(false, "2"));
  expect(view.container).toBeEmptyDOMElement();
  const before = [...native.reads];
  for (const revision of ["3", "4", "5"])
    await act(async () => view.rerender(tree(false, revision)));
  expect(native.reads).toEqual(before);
  view.rerender(tree(true, "5"));
  await waitFor(() =>
    expect(
      screen.getByRole("combobox", { name: "Chat for browser tab" }),
    ).toHaveValue("chat-2"),
  );
  await screen.findByRole("button", { name: "Detach tab from chat" });
  await waitFor(() => expect(native.reads.length).toBe(before.length + 1));
});

it("refreshes native detachment on a new attachment epoch without losing the chosen chat", async () => {
  const tree = (epoch: number) => (
    <BrowserToolbarPageActions
      projectId="project-1"
      tabId="browser-1"
      ready
      pageRevision="1"
      chats={[
        { id: "chat-1", title: "First" },
        { id: "chat-2", title: "Second" },
      ]}
      operation={null}
      attachmentEpoch={epoch}
      onStopped={() => undefined}
      onError={() => undefined}
    />
  );
  const view = render(tree(0));
  fireEvent.change(
    screen.getByRole("combobox", { name: "Chat for browser tab" }),
    { target: { value: "chat-2" } },
  );
  fireEvent.click(screen.getByRole("button", { name: "Attach tab to chat" }));
  await screen.findByRole("button", { name: "Detach tab from chat" });
  native.attached = null;
  view.rerender(tree(1));
  await screen.findByRole("button", { name: "Attach tab to chat" });
  expect(
    screen.getByRole("combobox", { name: "Chat for browser tab" }),
  ).toHaveValue("chat-2");
});

it("keeps a page capture pending through sleep and attaches its result to the originating Chat", async () => {
  useWorkbenchStore.setState({
    project: { id: "project-1", name: "Project", roots: [] },
    tabs: [
      {
        id: "chat-1",
        kind: "chat",
        title: "Chat",
        position: 0,
        pinned: false,
        dirty: false,
      },
    ],
    activeTabId: "browser-1",
  });
  pinAgentChatOwner("chat-1");
  let finish!: (value: unknown) => void;
  native.capture = new Promise((resolve) => {
    finish = resolve;
  });
  const tree = (visible: boolean) => (
    <BrowserToolbarPageActions
      visible={visible}
      projectId="project-1"
      tabId="browser-1"
      ready
      pageRevision="1"
      chats={[{ id: "chat-1", title: "Chat" }]}
      operation={null}
      attachmentEpoch={0}
      onStopped={() => undefined}
      onError={() => undefined}
    />
  );
  const view = render(tree(true));
  fireEvent.click(screen.getByRole("button", { name: "Ask about this page" }));
  await act(async () => undefined);
  view.rerender(tree(false));
  expect(view.container).toBeEmptyDOMElement();
  view.rerender(tree(true));
  expect(screen.getByRole("button", { name: /Attaching page/ })).toBeDisabled();
  const page = {
    kind: "browserPage",
    tabId: "browser-1",
    url: "https://example.com/",
    title: "Example",
    text: "An article",
  };
  await act(async () => finish(page));
  await waitFor(() =>
    expect(
      useWorkbenchStore.getState().chatUiById["chat-1"].attachedContext,
    ).toEqual([page]),
  );
  expect(
    screen.getByRole("button", { name: "Ask about this page" }),
  ).toBeEnabled();
});

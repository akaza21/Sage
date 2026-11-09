import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useChatAccessDrafts } from "../../app/chatAccessDrafts";
import { useSessionAccessModeStore } from "../../app/sessionAccessModeStore";
import type { SessionAccessModeCapability } from "../../domain/sessionAccessMode";
import { ComposerAccessControl } from "./ComposerAccessControl";

const port = vi.hoisted(() => ({
  get: vi.fn(),
  set: vi.fn(),
  prepare: vi.fn(),
  grant: vi.fn(),
}));
vi.mock("../../ports/chatAccessChanges", () => ({
  chatAccessChanges: () => port,
}));

const codex: SessionAccessModeCapability[] = [
  {
    id: "readOnly",
    label: "Read only",
    description: "The agent reads files and asks before changes.",
    fallback: true,
  },
  {
    id: "ask",
    label: "Ask each time",
    description: "Review each permission request.",
  },
  {
    id: "workspaceWrite",
    label: "Workspace write",
    description: "The agent edits files in this workspace.",
  },
  {
    id: "fullAccess",
    label: "Full access",
    description: "Everything runs without asking.",
  },
];

/** A brand-new chat: its session does not exist yet. */
function renderNewChat(chatId?: string) {
  return render(
    <ComposerAccessControl
      bindingId="codex-acp"
      chatId={chatId}
      capabilities={codex}
      sessionConfig={null}
      open
      onToggle={() => {}}
      onDismiss={() => {}}
      disabled={false}
    />,
  );
}
const chip = () => screen.getByRole("button", { name: "Access options" });
const option = (name: string) => screen.findByRole("menuitemradio", { name });

beforeEach(() => {
  for (const fn of Object.values(port)) fn.mockReset();
  useSessionAccessModeStore.setState({ modes: {}, ready: true, error: null });
  useChatAccessDrafts.setState({ drafts: {} });
});
afterEach(cleanup);

it("lets a new chat choose its access before the first message", async () => {
  renderNewChat("chat-1");
  expect(chip()).toHaveTextContent("Read only");
  const ask = await option("Ask each time");
  expect(ask).toHaveAttribute("aria-disabled", "false");
  fireEvent.click(ask);
  await waitFor(() => expect(chip()).toHaveTextContent("Ask each time"));
  expect(useChatAccessDrafts.getState().drafts).toEqual({
    "chat-1": { bindingId: "codex-acp", mode: "ask" },
  });
  // Nothing reaches the native side until the session exists.
  expect(port.set).not.toHaveBeenCalled();
  // The harness default for other and new chats is untouched.
  expect(useSessionAccessModeStore.getState().modes).toEqual({});
});

it("confirms an elevated draft the way a live chat's menu does", async () => {
  renderNewChat("chat-1");
  fireEvent.click(await option("Workspace write"));
  expect(
    await screen.findByRole("heading", { name: "Switch to Workspace write?" }),
  ).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(useChatAccessDrafts.getState().drafts).toEqual({});
  expect(chip()).toHaveTextContent("Read only");

  cleanup();
  renderNewChat("chat-1");
  fireEvent.click(await option("Workspace write"));
  fireEvent.click(await screen.findByRole("button", { name: "Switch access" }));
  await waitFor(() => expect(chip()).toHaveTextContent("Workspace write"));
  expect(port.set).not.toHaveBeenCalled();
});

it("keeps permissive modes for a session that can confirm them natively", async () => {
  renderNewChat("chat-1");
  await option("Read only");
  expect(
    screen.queryByRole("menuitemradio", { name: "Full access" }),
  ).toBeNull();
});

it("clears the draft when the chat goes back to its default", async () => {
  useChatAccessDrafts.setState({
    drafts: { "chat-1": { bindingId: "codex-acp", mode: "ask" } },
  });
  renderNewChat("chat-1");
  expect(chip()).toHaveTextContent("Ask each time");
  fireEvent.click(await option("Read only"));
  await waitFor(() => expect(chip()).toHaveTextContent("Read only"));
  expect(useChatAccessDrafts.getState().drafts).toEqual({});
});

it("shows another harness's draft as nothing for this one", async () => {
  useChatAccessDrafts.setState({
    drafts: { "chat-1": { bindingId: "claude-agent-acp", mode: "ask" } },
  });
  renderNewChat("chat-1");
  expect(chip()).toHaveTextContent("Read only");
});

it("offers nothing to choose without a chat to hold the draft", async () => {
  renderNewChat();
  const ask = await option("Ask each time");
  expect(ask).toHaveAttribute("aria-disabled", "true");
  fireEvent.click(ask);
  expect(useChatAccessDrafts.getState().drafts).toEqual({});
});

import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useChatAccessDrafts } from "../../app/chatAccessDrafts";
import { useChatAccessStore } from "../../app/sessionAccessActions";
import { useSessionAccessModeStore } from "../../app/sessionAccessModeStore";
import type { AcpChatPage } from "../../domain/acp";
import type { AcpSessionConfig } from "../../domain/acpSessionConfig";
import type { SessionAccessModeCapability } from "../../domain/sessionAccessMode";
import { ComposerAccessControl } from "./ComposerAccessControl";
import { endedSession } from "./endedSession";

const port = vi.hoisted(() => ({
  get: vi.fn(),
  set: vi.fn(),
  prepare: vi.fn(),
  grant: vi.fn(),
}));
vi.mock("../../ports/chatAccessChanges", () => ({
  chatAccessChanges: () => port,
}));

const pi: SessionAccessModeCapability[] = [
  {
    id: "readOnly",
    label: "Read-only",
    description: "Sage declines every Pi write and shell command.",
    fallback: true,
  },
  {
    id: "workspaceWrite",
    label: "Workspace edits",
    description: "Ask before each workspace edit; shell commands are denied.",
  },
  {
    id: "fullAccess",
    label: "Full access",
    description: "Everything runs without asking.",
  },
];

/** The failed session's last config, shown while the chat has no head. */
const restored: AcpSessionConfig = {
  sageAgentSessionId: "pi-failed",
  runtimeEpoch: "",
  configRevision: "",
  modes: null,
  configOptions: [],
  restored: "Restored from this chat's last session.",
};

/** A Pi chat whose session failed: recovery removed its head. */
function renderAfterFailure(
  ended = { id: "pi-failed", bindingId: "pi-rpc" },
  sessionConfig: AcpSessionConfig | null = restored,
) {
  render(
    <ComposerAccessControl
      bindingId="pi-rpc"
      chatId="chat-1"
      endedSession={ended}
      capabilities={pi}
      sessionConfig={sessionConfig}
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
  port.get.mockImplementation(async (session: string) =>
    session === "pi-failed" ? "workspaceWrite" : "fullAccess",
  );
  useSessionAccessModeStore.setState({
    modes: { "pi-rpc": "readOnly" },
    ready: true,
    error: null,
  });
  useChatAccessStore.setState({ modes: {}, error: null });
  useChatAccessDrafts.setState({ drafts: {} });
});
afterEach(cleanup);

it("shows the access the failed session hands the next one", async () => {
  renderAfterFailure();
  await waitFor(() => expect(chip()).toHaveTextContent("Workspace edits"));
  expect(port.get).toHaveBeenCalledWith("pi-failed");
  // A narrower pick is drafted for the next session, never written to the
  // failed one.
  fireEvent.click(await option("Read-only"));
  await waitFor(() => expect(chip()).toHaveTextContent("Read-only"));
  expect(useChatAccessDrafts.getState().drafts).toEqual({
    "chat-1": { bindingId: "pi-rpc", mode: "readOnly" },
  });
  expect(port.set).not.toHaveBeenCalled();
});

it("needs no draft to keep what the failed session hands on", async () => {
  useChatAccessDrafts.setState({
    drafts: { "chat-1": { bindingId: "pi-rpc", mode: "readOnly" } },
  });
  renderAfterFailure(undefined, null);
  expect(chip()).toHaveTextContent("Read-only");
  fireEvent.click(await option("Workspace edits"));
  fireEvent.click(await screen.findByRole("button", { name: "Switch access" }));
  await waitFor(() => expect(chip()).toHaveTextContent("Workspace edits"));
  expect(useChatAccessDrafts.getState().drafts).toEqual({});
});

it("keeps any pick as a draft until the failed session's access is read", async () => {
  port.get.mockReturnValue(new Promise(() => {}));
  renderAfterFailure();
  expect(chip()).toHaveTextContent("Choose access");
  fireEvent.click(await option("Read-only"));
  expect(useChatAccessDrafts.getState().drafts).toEqual({
    "chat-1": { bindingId: "pi-rpc", mode: "readOnly" },
  });
});

it("shows the default after Full access, which is confirmed again", async () => {
  renderAfterFailure({ id: "pi-full", bindingId: "pi-rpc" });
  await waitFor(() => expect(port.get).toHaveBeenCalledWith("pi-full"));
  expect(chip()).toHaveTextContent("Read-only");
});

it("hands nothing on from a session of another harness", () => {
  renderAfterFailure({ id: "pi-failed", bindingId: "codex-acp" });
  expect(chip()).toHaveTextContent("Read-only");
  expect(port.get).not.toHaveBeenCalled();
});

describe("endedSession", () => {
  const session = (id: string, ordinal: number, legacy = false) => ({
    id,
    ordinal,
    legacy,
    bindingId: "pi-rpc",
  });
  const page = (
    currentSessionId: string | null,
    sessions: ReturnType<typeof session>[],
  ) =>
    ({ currentSessionId, sessions }) as unknown as Pick<
      AcpChatPage,
      "currentSessionId" | "sessions"
    >;

  it("is the chat's newest own session once it has no head", () => {
    expect(
      endedSession(
        page(null, [session("a", 0), session("c", 2), session("b", 1)]),
      ),
    ).toEqual({ id: "c", bindingId: "pi-rpc" });
    expect(
      endedSession(page(null, [session("a", 0), session("imported", 1, true)])),
    ).toEqual({ id: "a", bindingId: "pi-rpc" });
  });

  it("is nothing while the chat has a head or never ran a session", () => {
    expect(endedSession(page("a", [session("a", 0)]))).toBeUndefined();
    expect(endedSession(page(null, []))).toBeUndefined();
  });
});

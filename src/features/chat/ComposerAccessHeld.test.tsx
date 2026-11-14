import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useChatAccessStore } from "../../app/sessionAccessActions";
import { useSessionAccessModeStore } from "../../app/sessionAccessModeStore";
import { resetWorkbenchPortsForTests } from "../../ports/workbenchRuntime";
import { ComposerAccessControl } from "./ComposerAccessControl";
import { heldAccessSession } from "./heldAccessSession";

const port = vi.hoisted(() => ({
  get: vi.fn(),
  set: vi.fn(),
  prepare: vi.fn(),
  grant: vi.fn(),
}));
vi.mock("../../ports/chatAccessChanges", () => ({
  chatAccessChanges: () => port,
}));

const capabilities = [
  {
    id: "readOnly" as const,
    label: "Read-only",
    description: "Sage declines every Pi write and shell command.",
    fallback: true,
  },
  {
    id: "workspaceWrite" as const,
    label: "Workspace edits",
    description:
      "Ask before each verified workspace edit; shell commands are denied.",
  },
];

/** A chat whose Pi session failed mid-turn: recovery removed its head. */
const afterCrash = {
  currentSessionId: null,
  sessions: [
    { id: "pi-old", ordinal: 0, bindingId: "pi-rpc" },
    { id: "codex", ordinal: 1, bindingId: "codex-app-server" },
    { id: "pi-failed", ordinal: 2, bindingId: "pi-rpc" },
  ],
} as unknown as Parameters<typeof heldAccessSession>[0];

function renderAfterCrash(offered = capabilities) {
  render(
    <ComposerAccessControl
      bindingId="pi-rpc"
      capabilities={offered}
      sessionConfig={null}
      heldSessionId={heldAccessSession(afterCrash, "pi-rpc")}
      open
      onToggle={() => {}}
      onDismiss={() => {}}
      disabled={false}
    />,
  );
}

beforeEach(() => {
  resetWorkbenchPortsForTests();
  for (const fn of Object.values(port)) fn.mockReset();
  port.get.mockImplementation((session: string) =>
    Promise.resolve(session === "pi-failed" ? "workspaceWrite" : null),
  );
  port.set.mockResolvedValue(undefined);
  useSessionAccessModeStore.setState({
    modes: { "pi-rpc": "readOnly" },
    ready: true,
    error: null,
  });
  useChatAccessStore.setState({ modes: {}, error: null });
});
afterEach(cleanup);

it("keeps showing the chat's Workspace edits after its session failed", async () => {
  renderAfterCrash();
  const trigger = screen.getByRole("button", { name: "Access options" });
  await waitFor(() => expect(trigger).toHaveTextContent("Workspace edits"));
  expect(port.get).toHaveBeenCalledWith("pi-failed");
  // A change before the next send is the chat's, held where its next
  // session takes it from.
  fireEvent.click(
    await screen.findByRole("menuitemradio", { name: "Read-only" }),
  );
  await waitFor(() =>
    expect(port.set).toHaveBeenCalledWith("pi-failed", "readOnly"),
  );
});

it("says so when the harness can no longer carry the chat's access", async () => {
  renderAfterCrash(capabilities.slice(0, 1));
  expect(
    await screen.findByText(
      "This chat's access mode is not available from this harness now. Choose another one before sending.",
    ),
  ).toBeVisible();
});

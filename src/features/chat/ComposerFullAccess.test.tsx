import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useSessionAccessModeStore } from "../../app/sessionAccessModeStore";
import type { SessionAccessModeCapability } from "../../domain/sessionAccessMode";
import {
  getWorkbenchPorts,
  resetWorkbenchPortsForTests,
} from "../../ports/workbenchRuntime";
import { ComposerAccessControl } from "./ComposerAccessControl";

const port = vi.hoisted(() => ({
  list: vi.fn(),
  prepare: vi.fn(),
  grant: vi.fn(),
  revoke: vi.fn(),
}));
vi.mock("../../ports/fullAccessSessions", () => ({
  MAX_FULL_ACCESS_SESSIONS: 64,
  listFullAccessSessions: () => port.list(),
}));
vi.mock("../../ports/sessionFullAccessPort", () => ({
  sessionFullAccessPort: () => port,
}));

const claude: SessionAccessModeCapability[] = [
  {
    id: "ask",
    label: "Ask each time",
    description: "Review each permission request.",
    fallback: true,
    sessionModeId: "default",
  },
  {
    id: "acceptEdits",
    label: "Accept edits",
    description: "File edits run without asking.",
    sessionModeId: "acceptEdits",
  },
  {
    id: "plan",
    label: "Plan",
    description: "Claude plans without changing files.",
    sessionModeId: "plan",
  },
  {
    id: "fullAccess",
    label: "Full access",
    description: "Everything runs without asking.",
    sessionModeId: "bypassPermissions",
  },
];
const session = {
  sessionId: "session-1",
  availableModeIds: ["default", "acceptEdits", "plan", "bypassPermissions"],
};

function renderControl(target: typeof session | null = session) {
  return render(
    <ComposerAccessControl
      bindingId="claude-agent-acp"
      capabilities={claude}
      sessionConfig={
        target && {
          sageAgentSessionId: target.sessionId,
          runtimeEpoch: "1",
          configRevision: "1",
          modes: {
            currentModeId: "default",
            availableModes: target.availableModeIds.map((id) => ({
              id,
              name: id,
            })),
          },
          configOptions: [],
        }
      }
      open
      onToggle={() => {}}
      onDismiss={() => {}}
      disabled={false}
    />,
  );
}
const chip = () => screen.getByRole("button", { name: "Access options" });

beforeEach(() => {
  resetWorkbenchPortsForTests();
  for (const fn of Object.values(port)) fn.mockReset();
  port.prepare.mockResolvedValue({ confirmToken: "token-1" });
  port.grant.mockResolvedValue(undefined);
  port.revoke.mockResolvedValue(undefined);
  useSessionAccessModeStore.setState({
    modes: {},
    fullAccessSessions: [],
    ready: true,
    error: null,
    fullAccessError: null,
  });
});
afterEach(cleanup);

it("confirms Full access with the token issued when the dialog opened", async () => {
  renderControl();
  expect(chip()).toHaveTextContent("Ask each time");
  fireEvent.click(
    await screen.findByRole("menuitemradio", { name: "Full access" }),
  );
  expect(
    await screen.findByRole("heading", {
      name: "Allow full access in this chat?",
    }),
  ).toBeVisible();
  const allow = screen.getByRole("button", { name: "Allow full access" });
  await waitFor(() => expect(allow).toBeEnabled());
  expect(port.prepare).toHaveBeenCalledTimes(1);
  expect(port.prepare).toHaveBeenCalledWith("session-1");
  expect(port.grant).not.toHaveBeenCalled();
  fireEvent.click(allow);
  await waitFor(() =>
    expect(port.grant).toHaveBeenCalledWith("session-1", "token-1"),
  );
  await waitFor(() => expect(chip()).toHaveTextContent("Full access"));
  expect(chip()).toHaveClass("composer__access-label--full");
  // Never stored as the harness default for other or new chats.
  expect(useSessionAccessModeStore.getState().modes).toEqual({});
});

it("grants nothing when the confirmation is cancelled", async () => {
  renderControl();
  fireEvent.click(
    await screen.findByRole("menuitemradio", { name: "Full access" }),
  );
  fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));
  expect(port.grant).not.toHaveBeenCalled();
  expect(chip()).toHaveTextContent("Ask each time");
});

it("keeps the previous mode and says why when the grant is refused", async () => {
  port.grant.mockRejectedValue(new Error("expired"));
  renderControl();
  fireEvent.click(
    await screen.findByRole("menuitemradio", { name: "Full access" }),
  );
  const allow = await screen.findByRole("button", {
    name: "Allow full access",
  });
  await waitFor(() => expect(allow).toBeEnabled());
  fireEvent.click(allow);
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Full access was not granted. Confirm again.",
  );
  expect(chip()).toHaveTextContent("Ask each time");
});

it("ends Full access before storing another mode, and only once it ended", async () => {
  useSessionAccessModeStore.setState({ fullAccessSessions: ["session-1"] });
  port.revoke.mockRejectedValueOnce(new Error("busy"));
  const { unmount } = renderControl();
  expect(chip()).toHaveTextContent("Full access");
  fireEvent.click(
    await screen.findByRole("menuitemradio", { name: "Accept edits" }),
  );
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Full access could not be turned off. Try again.",
  );
  expect(useSessionAccessModeStore.getState().modes).toEqual({});
  expect(chip()).toHaveTextContent("Full access");
  unmount();
  renderControl();
  fireEvent.click(
    await screen.findByRole("menuitemradio", { name: "Accept edits" }),
  );
  await waitFor(() => expect(chip()).toHaveTextContent("Accept edits"));
  expect(port.revoke).toHaveBeenLastCalledWith("session-1");
  expect(useSessionAccessModeStore.getState().modes).toEqual({
    "claude-agent-acp": "acceptEdits",
  });
  await useSessionAccessModeStore.getState().settle();
  expect(await getWorkbenchPorts().settings.getSessionAccessModes()).toEqual({
    "claude-agent-acp": "acceptEdits",
  });
});

it("offers only advertised modes, and Full access only to a live session", async () => {
  renderControl({ ...session, availableModeIds: ["default", "acceptEdits"] });
  expect(
    (await screen.findAllByRole("menuitemradio")).map((item) =>
      item.getAttribute("aria-label"),
    ),
  ).toEqual(["Ask each time", "Accept edits"]);
  cleanup();
  renderControl(null);
  await screen.findByRole("menuitemradio", { name: "Plan" });
  expect(
    screen.queryByRole("menuitemradio", { name: "Full access" }),
  ).toBeNull();
});

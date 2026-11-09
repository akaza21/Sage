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
import type { SessionAccessModeCapability } from "../../domain/sessionAccessMode";
import { resetWorkbenchPortsForTests } from "../../ports/workbenchRuntime";
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
    id: "auto",
    label: "Auto",
    description: "Claude runs what it judges safe.",
    sessionModeId: "auto",
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
  availableModeIds: [
    "default",
    "acceptEdits",
    "plan",
    "auto",
    "bypassPermissions",
  ],
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
  port.get.mockResolvedValue(null);
  port.set.mockResolvedValue(undefined);
  port.prepare.mockResolvedValue({ confirmToken: "token-1" });
  port.grant.mockResolvedValue(undefined);
  useSessionAccessModeStore.setState({
    modes: {},
    ready: true,
    error: null,
  });
  useChatAccessStore.setState({ modes: {}, error: null });
});
afterEach(cleanup);

async function confirm(label: string, heading: string, button: string) {
  fireEvent.click(await screen.findByRole("menuitemradio", { name: label }));
  expect(await screen.findByRole("heading", { name: heading })).toBeVisible();
  const allow = screen.getByRole("button", { name: button });
  await waitFor(() => expect(allow).toBeEnabled());
  return allow;
}

it("confirms Full access with the token issued for this chat and mode", async () => {
  renderControl();
  expect(chip()).toHaveTextContent("Ask each time");
  const allow = await confirm(
    "Full access",
    "Allow full access in this chat?",
    "Allow full access",
  );
  expect(port.prepare).toHaveBeenCalledTimes(1);
  expect(port.prepare).toHaveBeenCalledWith("session-1", "fullAccess");
  expect(port.grant).not.toHaveBeenCalled();
  fireEvent.click(allow);
  await waitFor(() =>
    expect(port.grant).toHaveBeenCalledWith(
      "session-1",
      "fullAccess",
      "token-1",
    ),
  );
  await waitFor(() => expect(chip()).toHaveTextContent("Full access"));
  expect(chip()).toHaveClass("composer__access-label--full");
  // Never stored as the harness default for other or new chats.
  expect(useSessionAccessModeStore.getState().modes).toEqual({});
});

it("confirms Auto too, and says what it lets run", async () => {
  renderControl();
  const allow = await confirm("Auto", "Allow auto in this chat?", "Allow auto");
  expect(screen.getByText(/Sage does not review them/)).toBeVisible();
  fireEvent.click(allow);
  await waitFor(() =>
    expect(port.grant).toHaveBeenCalledWith("session-1", "auto", "token-1"),
  );
  await waitFor(() => expect(chip()).toHaveTextContent("Auto"));
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

it("shows what the chat really holds after a refused grant", async () => {
  port.grant.mockRejectedValue(new Error("expired"));
  renderControl();
  const allow = await confirm(
    "Full access",
    "Allow full access in this chat?",
    "Allow full access",
  );
  port.get.mockResolvedValue("plan");
  fireEvent.click(allow);
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "This access was not granted. Confirm again.",
  );
  await waitFor(() => expect(chip()).toHaveTextContent("Plan"));
  expect(port.get).toHaveBeenLastCalledWith("session-1");
});

it("applies a pick to this chat only, and steps down natively", async () => {
  // The window has not seen this chat's Full access yet: stepping down
  // must not depend on it.
  port.get.mockReturnValue(new Promise(() => {}));
  renderControl();
  fireEvent.click(
    await screen.findByRole("menuitemradio", { name: "Accept edits" }),
  );
  await waitFor(() =>
    expect(port.set).toHaveBeenCalledWith("session-1", "acceptEdits"),
  );
  await waitFor(() => expect(chip()).toHaveTextContent("Accept edits"));
  // The harness default for other and new chats is untouched.
  expect(useSessionAccessModeStore.getState().modes).toEqual({});
});

it("says so when stepping down stopped a running turn", async () => {
  port.get.mockResolvedValue("fullAccess");
  port.set.mockResolvedValue({ turnCancelled: true });
  renderControl();
  await waitFor(() => expect(chip()).toHaveTextContent("Full access"));
  fireEvent.click(await screen.findByRole("menuitemradio", { name: "Plan" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "The running turn was stopped because this chat's access was lowered.",
  );
  expect(chip()).toHaveTextContent("Plan");
});

it("keeps the chat's mode and says why when a pick is refused", async () => {
  port.get.mockResolvedValue("fullAccess");
  port.set.mockRejectedValue(new Error("busy"));
  renderControl();
  await waitFor(() => expect(chip()).toHaveTextContent("Full access"));
  fireEvent.click(
    await screen.findByRole("menuitemradio", { name: "Accept edits" }),
  );
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "This chat's access could not be changed. Try again.",
  );
  expect(chip()).toHaveTextContent("Full access");
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
  const plan = await screen.findByRole("menuitemradio", { name: "Plan" });
  expect(
    screen.queryByRole("menuitemradio", { name: "Full access" }),
  ).toBeNull();
  // Without a live session nothing here changes the chat.
  expect(plan).toHaveAttribute("aria-disabled", "true");
  fireEvent.click(plan);
  expect(port.set).not.toHaveBeenCalled();
});

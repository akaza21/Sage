import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useState } from "react";
import { useSessionAccessModeStore } from "../../app/sessionAccessModeStore";
import {
  getWorkbenchPorts,
  resetWorkbenchPortsForTests,
} from "../../ports/workbenchRuntime";
import type { AcpSessionConfig } from "../../domain/acpSessionConfig";
import { ComposerAccessControl as Control } from "./ComposerAccessControl";

const port = vi.hoisted(() => ({
  get: vi.fn(),
  set: vi.fn(),
  prepare: vi.fn(),
  grant: vi.fn(),
}));
vi.mock("../../ports/chatAccessChanges", () => ({
  chatAccessChanges: () => port,
}));

/** A live chat whose harness reports no session modes. */
const liveChat: AcpSessionConfig = {
  sageAgentSessionId: "session-1",
  runtimeEpoch: "1",
  configRevision: "1",
  modes: null,
  configOptions: [],
};

// A pick applies to the live chat it is made in.
function ComposerAccessControl(
  props: Omit<Parameters<typeof Control>[0], "sessionConfig">,
) {
  return <Control sessionConfig={liveChat} {...props} />;
}

const capabilities = [
  {
    id: "readOnly" as const,
    label: "Read-only",
    description: "Read workspace files; no writes or network.",
  },
  {
    id: "workspaceWrite" as const,
    label: "Workspace edits",
    description:
      "Edits and commands run without asking inside the workspace and temp folders; no network access.",
  },
];

beforeEach(() => {
  resetWorkbenchPortsForTests();
  for (const fn of Object.values(port)) fn.mockReset();
  port.get.mockResolvedValue(null);
  port.set.mockResolvedValue(undefined);
  useSessionAccessModeStore.setState({
    modes: { fixture: "readOnly" },
    chatModes: {},
    ready: true,
    error: null,
    chatError: null,
  });
});
afterEach(cleanup);

it.each([
  ["readOnly", "Read-only", ".lucide-eye"],
  ["workspaceWrite", "Workspace edits", ".lucide-shield-check"],
] as const)(
  "uses the %s icon on both the chip and its selected row",
  async (mode, label, icon) => {
    useSessionAccessModeStore.setState({ modes: { fixture: mode } });
    render(
      <ComposerAccessControl
        bindingId="fixture"
        capabilities={capabilities}
        open
        onToggle={() => {}}
        onDismiss={() => {}}
        disabled={false}
      />,
    );
    expect(
      screen
        .getByRole("button", { name: "Access options" })
        .querySelector(icon),
    ).not.toBeNull();
    expect(
      (await screen.findByRole("menuitemradio", { name: label })).querySelector(
        icon,
      ),
    ).not.toBeNull();
  },
);

it("introduces the approval question and distinct icons only for admitted modes", async () => {
  render(
    <ComposerAccessControl
      bindingId="fixture"
      capabilities={[
        ...capabilities,
        {
          id: "ask",
          label: "Ask each time",
          description: "Review each permission request.",
        },
      ]}
      open
      onToggle={() => {}}
      onDismiss={() => {}}
      disabled={false}
    />,
  );
  expect(
    await screen.findByText(
      "How should agent actions be approved in this chat?",
    ),
  ).toBeVisible();
  const menu = screen.getByRole("menu", { name: "Access mode" });
  expect(menu).toHaveAccessibleDescription(
    "How should agent actions be approved in this chat?",
  );
  expect(
    screen.getByText("How should agent actions be approved in this chat?"),
  ).toHaveAttribute("aria-hidden", "true");
  expect(menu.querySelectorAll('[role="menuitemradio"]')).toHaveLength(3);
  expect(
    screen
      .getByRole("menuitemradio", { name: "Read-only" })
      .querySelector(".lucide-eye"),
  ).not.toBeNull();
  expect(
    screen
      .getByRole("menuitemradio", { name: "Workspace edits" })
      .querySelector(".lucide-shield-check"),
  ).not.toBeNull();
  expect(
    screen
      .getByRole("menuitemradio", { name: "Ask each time" })
      .querySelector(".lucide-message-circle-question-mark"),
  ).not.toBeNull();
  expect(menu.querySelectorAll(".lucide-check")).toHaveLength(1);
  expect(screen.queryByRole("link")).toBeNull();
});

it("shows admitted descriptions, checked selection, keyboard navigation and Escape focus return", async () => {
  const dismiss = vi.fn();
  function Control() {
    const [open, setOpen] = useState(true);
    return (
      <ComposerAccessControl
        bindingId="fixture"
        capabilities={capabilities}
        open={open}
        onToggle={() => setOpen(true)}
        onDismiss={() => {
          dismiss();
          setOpen(false);
        }}
        disabled={false}
      />
    );
  }
  render(<Control />);
  const read = await screen.findByRole("menuitemradio", { name: "Read-only" });
  const write = screen.getByRole("menuitemradio", { name: "Workspace edits" });
  expect(read).toHaveAttribute("aria-checked", "true");
  expect(read).toHaveAccessibleDescription(capabilities[0].description);
  expect(write).toHaveAccessibleDescription(capabilities[1].description);
  expect(screen.queryByRole("combobox")).toBeNull();
  const scope = screen.getByText(
    "Read workspace files; no writes or network. Applies to this chat from your next message.",
  );
  expect(scope.closest('[role="menu"]')).toBeNull();
  expect(
    screen.getByRole("button", { name: "Access options" }),
  ).toHaveAccessibleDescription(
    "Read workspace files; no writes or network. Applies to this chat from your next message.",
  );
  expect(
    Array.from(screen.getByRole("menu").children).map((element) =>
      element.getAttribute("role"),
    ),
  ).toEqual([null, "menuitemradio", "menuitemradio"]);
  expect(screen.queryByText("Ask each time")).toBeNull();
  expect(screen.queryByText("Full access")).toBeNull();
  await waitFor(() => expect(read).toHaveFocus());
  fireEvent.keyDown(read, { key: "End" });
  expect(write).toHaveFocus();
  fireEvent.keyDown(write, { key: "Home" });
  expect(read).toHaveFocus();
  fireEvent.keyDown(read, { key: "ArrowUp" });
  expect(write).toHaveFocus();
  fireEvent.keyDown(write, { key: "Escape" });
  expect(dismiss).toHaveBeenCalled();
  expect(screen.getByRole("button", { name: "Access options" })).toHaveFocus();
});

it("applies the confirmed policy to this chat only and returns focus to access", async () => {
  const save = vi.spyOn(getWorkbenchPorts().settings, "setSessionAccessModes");
  const dismiss = vi.fn();
  function Control() {
    const [open, setOpen] = useState(true);
    return (
      <ComposerAccessControl
        bindingId="fixture"
        capabilities={capabilities}
        open={open}
        onToggle={() => setOpen(true)}
        onDismiss={() => {
          dismiss();
          setOpen(false);
        }}
        disabled={false}
      />
    );
  }
  render(<Control />);
  fireEvent.click(
    await screen.findByRole("menuitemradio", { name: "Workspace edits" }),
  );
  const dialog = await screen.findByRole("dialog", {
    name: "Switch to Workspace edits?",
  });
  expect(save).not.toHaveBeenCalled();
  expect(dialog).toHaveTextContent(capabilities[1].description);
  expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
  const trigger = screen.getByRole("button", { name: "Access options" });
  const focus = vi.spyOn(trigger, "focus").mockImplementation(() => {
    expect(dialog).not.toHaveAttribute("open");
    HTMLButtonElement.prototype.focus.call(trigger);
  });
  fireEvent.click(screen.getByRole("button", { name: "Switch access" }));
  await waitFor(() =>
    expect(port.set).toHaveBeenCalledWith("session-1", "workspaceWrite"),
  );
  // Defaults for other and new chats are chosen in Settings.
  expect(save).not.toHaveBeenCalled();
  expect(dismiss).toHaveBeenCalled();
  expect(focus).toHaveBeenCalled();
  expect(screen.getByRole("button", { name: "Access options" })).toHaveFocus();
});

it("cancels workspace-write escalation without persisting, and drops it on binding changes", async () => {
  const save = vi.spyOn(getWorkbenchPorts().settings, "setSessionAccessModes");
  const dismiss = vi.fn();
  const props = {
    bindingId: "fixture",
    capabilities,
    open: true,
    onToggle: vi.fn(),
    onDismiss: dismiss,
    disabled: false,
  };
  const { rerender } = render(<ComposerAccessControl {...props} />);
  fireEvent.click(
    await screen.findByRole("menuitemradio", { name: "Workspace edits" }),
  );
  const dialog = await screen.findByRole("dialog");
  fireEvent.keyDown(dialog, { key: "Escape" });
  expect(save).not.toHaveBeenCalled();
  expect(dismiss).toHaveBeenCalled();
  rerender(<ComposerAccessControl {...props} open={false} />);
  rerender(<ComposerAccessControl {...props} />);
  fireEvent.click(
    await screen.findByRole("menuitemradio", { name: "Workspace edits" }),
  );
  await screen.findByRole("dialog");
  rerender(<ComposerAccessControl {...props} bindingId="other" />);
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(save).not.toHaveBeenCalled();
  expect(port.set).not.toHaveBeenCalled();
});

it.each(["disabled", "withdrawn"])(
  "discards a pending escalation when capabilities become %s",
  async (change) => {
    const save = vi.spyOn(
      getWorkbenchPorts().settings,
      "setSessionAccessModes",
    );
    const props = {
      bindingId: "fixture",
      capabilities,
      open: true,
      disabled: false,
      onToggle: vi.fn(),
      onDismiss: vi.fn(),
    };
    const { rerender } = render(<ComposerAccessControl {...props} />);
    fireEvent.click(
      await screen.findByRole("menuitemradio", { name: "Workspace edits" }),
    );
    await screen.findByRole("dialog");
    rerender(
      <ComposerAccessControl
        {...props}
        disabled={change === "disabled"}
        capabilities={
          change === "withdrawn" ? capabilities.slice(0, 1) : capabilities
        }
      />,
    );
    expect(screen.queryByRole("dialog")).toBeNull();
    rerender(<ComposerAccessControl {...props} />);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(save).not.toHaveBeenCalled();
    expect(port.set).not.toHaveBeenCalled();
  },
);

it("does not relabel an unsupported saved policy as an admitted permission mode", () => {
  render(
    <ComposerAccessControl
      bindingId="fixture"
      capabilities={[
        {
          id: "ask",
          label: "Ask each time",
          description: "Review each permission request; no filesystem sandbox.",
        },
      ]}
      open
      onToggle={() => {}}
      onDismiss={() => {}}
      disabled={false}
    />,
  );
  expect(
    screen.getByRole("button", { name: "Access options" }),
  ).toHaveTextContent("Choose access");
  expect(screen.queryByRole("menuitemradio", { name: "Read-only" })).toBeNull();
  expect(screen.queryByText("Approve for me")).toBeNull();
});

it("has no interactive access surface without native capabilities", () => {
  render(
    <ComposerAccessControl
      bindingId="fixture"
      open
      capabilities={undefined}
      onToggle={() => {}}
      onDismiss={() => {}}
      disabled={false}
    />,
  );
  expect(screen.queryByRole("button", { name: "Access options" })).toBeNull();
  expect(screen.queryByRole("menu")).toBeNull();
});

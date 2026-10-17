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
import { ComposerAccessControl } from "./ComposerAccessControl";

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
  useSessionAccessModeStore.setState({
    modes: { fixture: "readOnly" },
    ready: true,
    error: null,
  });
});
afterEach(cleanup);

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
  const read = screen.getByRole("menuitemradio", { name: "Read-only" });
  const write = screen.getByRole("menuitemradio", { name: "Workspace edits" });
  expect(read).toHaveAttribute("aria-checked", "true");
  expect(read).toHaveAccessibleDescription(capabilities[0].description);
  expect(write).toHaveAccessibleDescription(capabilities[1].description);
  expect(screen.queryByRole("combobox")).toBeNull();
  const scope = screen.getByText(
    "Read workspace files; no writes or network. Applies from your next message with this harness.",
  );
  expect(scope.closest('[role="menu"]')).toBeNull();
  expect(
    screen.getByRole("button", { name: "Access options" }),
  ).toHaveAccessibleDescription(
    "Read workspace files; no writes or network. Applies from your next message with this harness.",
  );
  expect(
    Array.from(screen.getByRole("menu").children).every(
      (element) => element.getAttribute("role") === "menuitemradio",
    ),
  ).toBe(true);
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

it("persists the selected policy and returns focus to access", async () => {
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
    screen.getByRole("menuitemradio", { name: "Workspace edits" }),
  );
  await waitFor(() =>
    expect(save).toHaveBeenCalledWith({ fixture: "workspaceWrite" }),
  );
  expect(dismiss).toHaveBeenCalled();
  expect(screen.getByRole("button", { name: "Access options" })).toHaveFocus();
});

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

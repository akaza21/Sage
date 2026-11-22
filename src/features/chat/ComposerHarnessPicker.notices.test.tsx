import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it } from "vitest";
import { useState } from "react";
import { useWorkbenchStore } from "../../app/store";
import { ComposerHarnessPicker } from "./ComposerHarnessPicker";
import type { HarnessDefinition } from "../../domain/harness";
import type { HarnessDiscoveryPhase } from "../../app/harnessSlice";

const initial = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initial, true);
});
const harness: HarnessDefinition = {
  id: "fixture",
  name: "Fixture",
  description: "ACP",
  source: "builtIn",
  transport: "acp",
  bindingId: "fixture-binding",
  installState: "available",
  accountModelDiscovery: true,
};
beforeEach(() => useWorkbenchStore.setState({ selectedHarnessId: harness.id }));
function Picker({
  discoveryPhase,
}: { discoveryPhase?: HarnessDiscoveryPhase } = {}) {
  const [open, setOpen] = useState(false);
  return (
    <ComposerHarnessPicker
      discoveryPhase={discoveryPhase}
      open={open}
      onOpenChange={setOpen}
      candidates={[{ id: harness.id, label: harness.name }]}
      selectedId={harness.id}
      onSelect={() => {}}
    />
  );
}

it.each([
  ["checking", /Checking the Fixture account/],
  ["consent", /stores credentials/],
  ["waiting", /Finish signing in/],
] as const)(
  "shows global %s even when the existing session is ready to prompt",
  async (phase, message) => {
    useWorkbenchStore.setState({
      harnesses: [harness],
      harnessDiscoveryPhase: phase,
    });
    render(<Picker discoveryPhase="ready" />);
    fireEvent.click(screen.getByRole("button", { name: "Harness" }));
    expect(await screen.findByRole("status")).toHaveTextContent(message);
  },
);

it("keeps another harness's account consent out of this chat's picker", async () => {
  useWorkbenchStore.setState({
    harnesses: [
      harness,
      { ...harness, id: "other", bindingId: "other-binding" },
    ],
    selectedHarnessId: "other",
    harnessDiscoveryPhase: "consent",
  });
  render(<Picker discoveryPhase="ready" />);
  fireEvent.click(screen.getByRole("button", { name: "Harness" }));
  await screen.findByRole("dialog", { name: "Harness" });
  expect(screen.queryByText(/stores credentials/)).toBeNull();
  expect(screen.queryByRole("status")).toBeNull();
});

it.each([
  "idle",
  "ready",
  "checking",
  "signInRequired",
  "consent",
  "waiting",
  "error",
] as const)(
  "keeps the binding explanation owned by its picker during %s",
  async (phase) => {
    useWorkbenchStore.setState({
      harnesses: [
        {
          ...harness,
          installState: "unavailable",
          bindingId: null,
          unavailableReason: "adapterAbsent",
        },
      ],
      harnessDiscoveryPhase: phase,
    });
    const { container } = render(<Picker />);
    expect(screen.queryByText(/Fixture isn't set up for Sage yet/)).toBeNull();
    expect(
      container.querySelector(".composer__notice, .composer__error"),
    ).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Harness" }));
    const notice = await screen.findByRole("alert");
    expect(screen.getAllByRole("alert")).toHaveLength(1);
    expect(notice).toHaveTextContent("Fixture isn't set up for Sage yet.");
    expect(
      screen.queryByText(/stores credentials|Checking the Fixture account/),
    ).toBeNull();
  },
);
it("keeps consent in the popover and unmounts it when closed", async () => {
  useWorkbenchStore.setState({
    harnesses: [harness],
    harnessDiscoveryPhase: "consent",
  });
  const { container } = render(<Picker />);
  expect(screen.queryByText(/stores credentials/)).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Harness" }));
  const message = await screen.findByText(/stores credentials/);
  expect(message.closest(".composer__harness-menu")).not.toBeNull();
  expect(container).not.toContainElement(message);
  expect(screen.getByRole("button", { name: "Continue" })).toBeVisible();
  expect(screen.getByRole("button", { name: "Cancel" })).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Harness" }));
  expect(screen.queryByText(/stores credentials/)).toBeNull();
});
it("shows checking as quiet picker progress without a warning in chat", async () => {
  useWorkbenchStore.setState({
    harnesses: [harness],
    harnessDiscoveryPhase: "checking",
  });
  const { container } = render(<Picker />);
  expect(
    container.querySelector(".notice--warning, .composer__notice"),
  ).toBeNull();
  expect(screen.queryByRole("status")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Harness" }));
  const status = await screen.findByRole("status");
  expect(status).toHaveClass("notice--info");
  expect(status).toHaveTextContent("Checking the Fixture account");
});
it("owns one missing-binding explanation with one recovery action", async () => {
  useWorkbenchStore.setState({
    harnesses: [
      {
        ...harness,
        installState: "unavailable",
        bindingId: null,
        unavailableReason: "adapterAbsent",
      },
      { ...harness, id: "available", name: "Available harness" },
    ],
    harnessDiscoveryPhase: "ready",
  });
  render(<Picker />);
  fireEvent.click(screen.getByRole("button", { name: "Harness" }));
  const notice = await screen.findByRole("alert");
  expect(notice).toHaveTextContent("Fixture isn't set up for Sage yet");
  expect(within(notice).getAllByRole("button")).toHaveLength(1);
  expect(
    within(notice).getByRole("button", { name: "Open Settings" }),
  ).toBeVisible();
  expect(screen.getAllByText(/Fixture isn't set up for Sage yet/)).toHaveLength(
    1,
  );
});

it("keyboard opening focuses the selected non-first harness", async () => {
  const first = { ...harness, id: "first", name: "First" };
  useWorkbenchStore.setState({
    harnesses: [first, harness],
    harnessDiscoveryPhase: "ready",
  });
  function Selection() {
    const [open, setOpen] = useState(false);
    const [selectedId, select] = useState(harness.id);
    return (
      <ComposerHarnessPicker
        open={open}
        onOpenChange={setOpen}
        candidates={[
          { id: first.id, label: first.name },
          { id: harness.id, label: harness.name },
        ]}
        selectedId={selectedId}
        onSelect={select}
      />
    );
  }
  render(<Selection />);
  const trigger = screen.getByRole("button", { name: "Harness" });
  trigger.focus();
  fireEvent.keyDown(trigger, { key: "ArrowDown" });
  const selected = await screen.findByRole("option", { name: "Fixture" });
  await waitFor(() => expect(selected).toHaveFocus());
  fireEvent.click(selected);
  expect(trigger).toHaveAttribute("data-selected-id", harness.id);
});

it("keeps build and release dismissals across close and reopen but shows new identities", async () => {
  const current = {
    ...harness,
    boundVersion: "1.2.3",
    characterisation: "uncharacterised" as const,
    update: { latestVersion: "1.3.0", command: "update fixture" },
  };
  useWorkbenchStore.setState({
    harnesses: [current],
    harnessDiscoveryPhase: "ready",
  });
  render(<Picker />);
  const trigger = screen.getByRole("button", { name: "Harness" });
  fireEvent.click(trigger);
  fireEvent.click(
    await screen.findByRole("button", { name: "Dismiss build notice" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Dismiss update" }));
  fireEvent.click(trigger);
  expect(screen.queryByRole("dialog", { name: "Harness" })).toBeNull();
  fireEvent.click(trigger);
  await screen.findByRole("dialog", { name: "Harness" });
  expect(
    screen.queryByText(/hasn't been tested|1.3.0 is available/),
  ).toBeNull();
  useWorkbenchStore.setState({
    harnesses: [
      {
        ...current,
        boundVersion: "1.2.4",
        update: { ...current.update, latestVersion: "1.4.0" },
      },
    ],
  });
  expect(
    await screen.findByText(/Fixture 1.2.4 hasn't been tested/),
  ).toBeVisible();
  expect(screen.getByText("Fixture 1.4.0 is available.")).toBeVisible();
});

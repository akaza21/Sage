import {
  cleanup,
  fireEvent,
  render,
  screen,
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
    expect(screen.queryByText(/ACP adapter isn't installed/)).toBeNull();
    expect(
      container.querySelector(".composer__notice, .composer__error"),
    ).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Harness" }));
    const notice = await screen.findByRole("alert");
    expect(screen.getAllByRole("alert")).toHaveLength(1);
    expect(notice).toHaveTextContent(
      "This harness's ACP adapter isn't installed.",
    );
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
    ],
    harnessDiscoveryPhase: "ready",
  });
  render(<Picker />);
  fireEvent.click(screen.getByRole("button", { name: "Harness" }));
  const notice = await screen.findByRole("alert");
  expect(notice).toHaveTextContent("ACP adapter isn't installed");
  expect(within(notice).getAllByRole("button")).toHaveLength(1);
  expect(screen.getAllByText(/ACP adapter isn't installed/)).toHaveLength(1);
});

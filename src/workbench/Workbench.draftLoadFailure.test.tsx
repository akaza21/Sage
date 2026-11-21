import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../app/store";
import { resetBrowserDesktopForTests } from "../services/browserDesktop";
import { settleLazyModules } from "../test/settleLazyModules";
import { AgentChatPanes } from "./AgentChatPanes";
import { AgentNewChatButton } from "./AgentCollapsedNavigation";
import { AgentCenterKeeper } from "./WorkbenchContinuity";

// Isolate actions-chunk failures from already cached modules.
const actionsLoad = vi.hoisted(() => ({ fail: true }));
vi.mock("../app/newChatDraftActions", async (importOriginal) => {
  if (actionsLoad.fail)
    throw new Error("Failed to fetch dynamically imported module");
  return importOriginal();
});

const initial = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initial, true);
  resetBrowserDesktopForTests();
});

function Center() {
  return (
    <>
      <AgentNewChatButton />
      <AgentCenterKeeper windowMode="agent" />
      <AgentChatPanes />
    </>
  );
}

it("explains a failed automatic draft load without retrying or an unhandled rejection", async () => {
  resetBrowserDesktopForTests();
  await useWorkbenchStore.getState().initialize();
  await useWorkbenchStore.getState().createProject({
    name: "Empty project",
    sourceTokens: [],
  });
  let attempts = 0;
  const unsubscribe = useWorkbenchStore.subscribe((state, previous) => {
    if (state.newChatDraftOpening && !previous.newChatDraftOpening) {
      attempts += 1;
      // Fail immediately on a runaway load, before it can starve the runner.
      expect(attempts).toBeLessThanOrEqual(1);
    }
  });
  const unhandled: unknown[] = [];
  const onUnhandled = (reason: unknown) => unhandled.push(reason);
  process.on("unhandledRejection", onUnhandled);
  try {
    const mounted = render(<Center />);
    await settleLazyModules();
    expect(
      screen.queryByRole("alert", { name: "New chat failed to load" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getAllByText(
        "Couldn't open a new chat. Click New chat to try again.",
      ),
    ).toHaveLength(1);
    expect(
      screen.queryByText(
        "The selected harness cannot accept prompts in this session.",
      ),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
    expect(attempts).toBe(1);
    expect(useWorkbenchStore.getState().newChatDraft).toBeNull();

    mounted.unmount();
    render(<Center />);
    await settleLazyModules();
    expect(attempts).toBe(1);

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    expect(unhandled).toEqual([]);
  } finally {
    unsubscribe();
    process.off("unhandledRejection", onUnhandled);
  }
});

it("opens an editable draft through New chat after a transient actions-load failure", async () => {
  resetBrowserDesktopForTests();
  await useWorkbenchStore.getState().initialize();
  await useWorkbenchStore.getState().createProject({
    name: "Empty project",
    sourceTokens: [],
  });
  useWorkbenchStore.setState({
    harnesses: [
      {
        id: "draft-harness",
        name: "Draft harness",
        description: "Fixture ACP harness",
        source: "builtIn",
        transport: "acp",
        installState: "available",
        bindingId: "draft-binding",
        accountModelDiscovery: false,
      },
    ],
    selectedHarnessId: "draft-harness",
    harnessDiscoveryPhase: "ready",
    models: [{ id: "model-a", name: "Model A", isDefault: true }],
    selectedModelId: "model-a",
  });
  render(<Center />);
  await settleLazyModules();
  expect(
    screen.getByText("Couldn't open a new chat. Click New chat to try again."),
  ).toBeInTheDocument();

  actionsLoad.fail = false;
  fireEvent.click(screen.getByRole("button", { name: "New chat" }));
  await settleLazyModules();

  const draft = screen.getByRole("region", { name: "New chat draft" });
  const composer = within(draft).getByRole("combobox", {
    name: "Message agent",
  });
  expect(composer).toBeEnabled();
  fireEvent.change(composer, { target: { value: "Recovered prompt" } });
  expect(composer).toHaveValue("Recovered prompt");
  expect(
    within(draft).getByRole("button", { name: "Send message" }),
  ).toBeEnabled();
  expect(
    screen.queryByText(
      "Couldn't open a new chat. Click New chat to try again.",
    ),
  ).not.toBeInTheDocument();
  expect(
    screen.queryByText(
      "The selected harness cannot accept prompts in this session.",
    ),
  ).not.toBeInTheDocument();
});

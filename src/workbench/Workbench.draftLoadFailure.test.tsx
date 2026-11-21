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
import { AgentCenterKeeper } from "./WorkbenchContinuity";

// Isolate a persistent actions-chunk failure from already cached modules.
vi.mock("../app/newChatDraftActions", () => {
  throw new Error("Failed to fetch dynamically imported module");
});

const initial = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initial, true);
  resetBrowserDesktopForTests();
});

it("stops after one failed automatic draft load and waits for explicit Retry", async () => {
  resetBrowserDesktopForTests();
  await useWorkbenchStore.getState().initialize();
  await useWorkbenchStore.getState().createProject({
    name: "Empty project",
    sourceTokens: [],
  });
  let attempts = 0;
  let allowedAttempts = 1;
  const unsubscribe = useWorkbenchStore.subscribe((state, previous) => {
    if (state.newChatDraftOpening && !previous.newChatDraftOpening) {
      attempts += 1;
      // Fail immediately on a runaway load, before it can starve the runner.
      expect(attempts).toBeLessThanOrEqual(allowedAttempts);
    }
  });
  const unhandled: unknown[] = [];
  const onUnhandled = (reason: unknown) => unhandled.push(reason);
  process.on("unhandledRejection", onUnhandled);
  function Center() {
    return (
      <>
        <AgentCenterKeeper windowMode="agent" />
        <AgentChatPanes />
      </>
    );
  }

  try {
    const mounted = render(<Center />);
    const failure = await screen.findByRole("alert", {
      name: "New chat failed to load",
    });
    await settleLazyModules();
    expect(failure).toHaveTextContent("New chat could not be loaded.");
    expect(attempts).toBe(1);
    expect(useWorkbenchStore.getState().newChatDraft).toBeNull();

    mounted.unmount();
    render(<Center />);
    await settleLazyModules();
    expect(attempts).toBe(1);

    allowedAttempts = 2;
    fireEvent.click(
      within(
        screen.getByRole("alert", { name: "New chat failed to load" }),
      ).getByRole("button", { name: "Retry" }),
    );
    await settleLazyModules();
    expect(attempts).toBe(2);
    expect(
      screen.getByRole("alert", { name: "New chat failed to load" }),
    ).toBeVisible();
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    expect(unhandled).toEqual([]);
  } finally {
    unsubscribe();
    process.off("unhandledRejection", onUnhandled);
  }
});

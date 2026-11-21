import { act, cleanup, render, screen } from "@testing-library/react";
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

it("stops after one failed automatic draft load without adding failure UI or an unhandled rejection", async () => {
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
    await settleLazyModules();
    expect(
      screen.queryByRole("alert", { name: "New chat failed to load" }),
    ).not.toBeInTheDocument();
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

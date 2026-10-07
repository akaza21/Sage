import { act, render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { harnessAvailability } from "../features/chat/harnessAvailability";
import type { HarnessDefinition } from "../domain/harness";
import type * as AcpDesktopModule from "../services/acpDesktop";
import { App } from "./App";
import { useWorkbenchStore } from "./store";

const listeners = new Map<
  string,
  (event: { payload: HarnessDefinition[] }) => void
>();
let closeRequested:
  ((event: { preventDefault: () => void }) => Promise<void>) | undefined;

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(
    async (
      event: string,
      listener: (value: { payload: HarnessDefinition[] }) => void,
    ) => {
      listeners.set(event, listener);
      return () => listeners.delete(event);
    },
  ),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    onCloseRequested: vi.fn(async (listener) => {
      closeRequested = listener;
      return () => undefined;
    }),
  }),
}));

vi.mock("../workbench/Workbench", () => ({
  Workbench: () => <main>Workbench</main>,
}));

vi.mock("../services/desktop", () => ({
  desktop: { listHarnesses: vi.fn(async () => [detected]) },
}));

const discoverCodex = vi.fn();

vi.mock("../services/acpDesktop", async (importOriginal) => {
  const actual = await importOriginal<typeof AcpDesktopModule>();
  return {
    ...actual,
    acpDesktop: {
      ...actual.acpDesktop,
      discoverCodex: (request: unknown) => discoverCodex(request),
    },
  };
});

const detecting: HarnessDefinition = {
  id: "codex",
  name: "Codex",
  description: "",
  source: "builtIn",
  transport: "acp",
  installState: "detecting",
  bindingId: null,
  accountModelDiscovery: true,
};

const discovered: HarnessDefinition = {
  ...detecting,
  installState: "available",
  bindingId: "codex-app-server",
};

const detected: HarnessDefinition = {
  ...detecting,
  installState: "notInstalled",
};

describe("App harness discovery event", () => {
  beforeEach(() => {
    listeners.clear();
    closeRequested = undefined;
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
    useWorkbenchStore.setState({
      harnesses: [detecting],
      initialize: async () => undefined,
      phase: "ready",
    });
  });

  it("replaces detecting picker state when discovery completes", async () => {
    performance.clearMarks();
    render(<App />);
    await waitFor(() =>
      expect(listeners.has("harness-discovery-changed-v1")).toBe(true),
    );
    useWorkbenchStore.setState({ harnesses: [detecting] });
    expect(
      harnessAvailability(useWorkbenchStore.getState().harnesses[0])
        .disabledReason,
    ).toBe("Detecting harnesses…");
    act(() =>
      listeners.get("harness-discovery-changed-v1")?.({
        payload: [discovered],
      }),
    );
    expect(useWorkbenchStore.getState().harnesses).toEqual([discovered]);
    expect(
      harnessAvailability(useWorkbenchStore.getState().harnesses[0])
        .disabledReason,
    ).toBeUndefined();
    expect(
      performance.getEntriesByName("sage-startup:harness-presence"),
    ).toHaveLength(1);
    expect(
      performance.getEntriesByName("sage-startup:harness-subscription-start"),
    ).toHaveLength(1);
    expect(
      performance.getEntriesByName("sage-startup:harness-subscription-ready"),
    ).toHaveLength(1);
  });

  it("reads the catalog after subscribing when completion was missed", async () => {
    render(<App />);
    await waitFor(() =>
      expect(useWorkbenchStore.getState().harnesses).toEqual([detected]),
    );
  });

  it("refreshes repository status after the initial projection is ready", async () => {
    const refreshRepository = vi.fn(async () => undefined);
    const initialize = vi.fn(async () => undefined);
    useWorkbenchStore.setState({ initialize, refreshRepository });

    render(<App />);

    await waitFor(() => expect(initialize).toHaveBeenCalled());
    await waitFor(() => expect(refreshRepository).toHaveBeenCalled());
  });

  it("marks bootstrap dispatch, resolution, and the first ready render", async () => {
    performance.clearMarks();
    const initialize = vi.fn(async () => undefined);
    useWorkbenchStore.setState({ initialize, phase: "ready" });

    render(<App />);

    await waitFor(() => expect(initialize).toHaveBeenCalled());
    await waitFor(() =>
      expect(
        performance.getEntriesByName("sage-startup:first-ready-render"),
      ).toHaveLength(1),
    );
    expect(
      performance.getEntriesByName("sage-startup:bootstrap-dispatch"),
    ).toHaveLength(1);
    expect(
      performance.getEntriesByName("sage-startup:bootstrap-resolved"),
    ).toHaveLength(1);
  });

  it("shows a close-stage failure and permits another close attempt", async () => {
    const closeWorkbench = vi
      .fn()
      .mockRejectedValueOnce(new Error("editor flush failed"))
      .mockResolvedValueOnce(undefined);
    useWorkbenchStore.setState({ closeWorkbench });
    const screen = render(<App />);
    await waitFor(() => expect(closeRequested).toBeTypeOf("function"));

    await act(async () => {
      await closeRequested?.({ preventDefault: vi.fn() });
    });
    expect(screen.getByRole("alert").textContent).toContain(
      "editor flush failed",
    );

    await act(async () => {
      await closeRequested?.({ preventDefault: vi.fn() });
    });
    expect(closeWorkbench).toHaveBeenCalledTimes(2);
  });

  it("shows a close-stage failure while the app is booting", async () => {
    useWorkbenchStore.setState({
      phase: "booting",
      closeWorkbench: vi
        .fn()
        .mockRejectedValue(new Error("Close editorFlush failed")),
    });
    const screen = render(<App />);
    await waitFor(() => expect(closeRequested).toBeTypeOf("function"));

    await act(async () => {
      await closeRequested?.({ preventDefault: vi.fn() });
    });
    expect(screen.getByRole("alert").textContent).toContain(
      "Close editorFlush failed",
    );
  });
});

describe("App harness selection after bootstrap", () => {
  beforeEach(() => {
    listeners.clear();
    discoverCodex.mockReset();
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
    useWorkbenchStore.setState({
      phase: "ready",
      nativeTransitionEpoch: 0,
      project: {
        id: "project-a",
        name: "A",
        roots: [
          {
            id: "root-a",
            name: "A",
            ordinal: 0,
            availability: "available",
            message: null,
          },
        ],
      },
      projectGeneration: 0,
      projectTransitioning: false,
      harnesses: [detecting],
      selectedHarnessId: null,
      models: [],
      selectedModelId: null,
      harnessRequestGeneration: 0,
      harnessDiscoveryPhase: "idle",
      harnessDiscoveryError: null,
      initialize: async () => undefined,
    });
  });

  it("selects a harness that only becomes available after bootstrap", async () => {
    discoverCodex.mockResolvedValue({
      rootId: "root-a",
      projectId: "project-a",
      projectGeneration: "0",
      bindingId: "codex-app-server",
      account: "signedIn",
      models: [{ id: "gpt-x", name: "GPT-X", isDefault: true }],
    });
    render(<App />);
    await waitFor(() =>
      expect(listeners.has("harness-discovery-changed-v1")).toBe(true),
    );
    expect(useWorkbenchStore.getState().selectedHarnessId).toBeNull();
    act(() =>
      listeners.get("harness-discovery-changed-v1")?.({
        payload: [discovered],
      }),
    );
    await waitFor(() => {
      expect(useWorkbenchStore.getState().selectedHarnessId).toBe("codex");
      expect(useWorkbenchStore.getState().selectedModelId).toBe("gpt-x");
    });
  });
});

import { act, render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { harnessAvailability } from "../features/chat/harnessAvailability";
import type { HarnessDefinition } from "../domain/harness";
import type * as AcpHarnessDesktopModule from "../services/acpHarnessDesktop";
import { App } from "./App";
import { useWorkbenchStore } from "./store";

const listeners = new Map<
  string,
  (event: { payload: HarnessDefinition[] }) => void
>();
let closeEventListener: (() => Promise<void>) | undefined;
let closeTarget: unknown;
const closeUnlisten = vi.fn();
const callbacks = new Map<number, (event: unknown) => void>();
let callbackId = 0;
const invoke = vi.fn(
  async (command: string, args?: Record<string, unknown>) => {
    if (command === "plugin:event|listen") {
      const event = args?.event as string;
      const listener = callbacks.get(args?.handler as number);
      if (!listener) throw new Error("Tauri callback was not registered");
      if (event === "tauri://close-requested") {
        closeEventListener = listener as () => Promise<void>;
        closeTarget = args?.target;
      } else {
        listeners.set(
          event,
          listener as (event: { payload: HarnessDefinition[] }) => void,
        );
      }
      return 1;
    }
    if (command === "plugin:event|unlisten") {
      listeners.delete(args?.event as string);
    }
    return undefined;
  },
);

vi.mock("../workbench/Workbench", () => ({
  default: () => <main>Workbench</main>,
}));

vi.mock("../services/desktop", () => ({
  desktop: { listHarnesses: vi.fn(async () => [detected]) },
}));

const discoverCodex = vi.fn();

vi.mock("../services/acpHarnessDesktop", async (importOriginal) => {
  const actual = await importOriginal<typeof AcpHarnessDesktopModule>();
  return {
    ...actual,
    acpHarnessDesktop: {
      ...actual.acpHarnessDesktop,
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
    closeEventListener = undefined;
    closeTarget = undefined;
    closeUnlisten.mockClear();
    invoke.mockClear();
    callbacks.clear();
    callbackId = 0;
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {
        metadata: { currentWindow: { label: "main" } },
        transformCallback: (callback: (event: unknown) => void) => {
          const id = ++callbackId;
          callbacks.set(id, callback);
          return id;
        },
        invoke,
      },
    });
    Object.defineProperty(window, "__TAURI_EVENT_PLUGIN_INTERNALS__", {
      configurable: true,
      value: {
        unregisterListener: (event: string) => {
          if (event === "tauri://close-requested") closeUnlisten();
        },
      },
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
    await waitFor(() => expect(closeEventListener).toBeTypeOf("function"));

    await act(async () => {
      await closeEventListener?.();
    });
    expect(screen.getByRole("alert").textContent).toContain(
      "editor flush failed",
    );

    await act(async () => {
      await closeEventListener?.();
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
    await waitFor(() => expect(closeEventListener).toBeTypeOf("function"));

    await act(async () => {
      await closeEventListener?.();
    });
    expect(screen.getByRole("alert").textContent).toContain(
      "Close editorFlush failed",
    );
  });

  it("handles close requests for the current window through the event port", async () => {
    const closeWorkbench = vi.fn(async () => undefined);
    useWorkbenchStore.setState({ closeWorkbench });
    render(<App />);

    await waitFor(() => expect(closeEventListener).toBeTypeOf("function"));
    expect(closeTarget).toEqual({ kind: "Window", label: "main" });
    await act(async () => {
      await closeEventListener?.();
    });

    expect(closeWorkbench).toHaveBeenCalledOnce();
    expect(invoke).not.toHaveBeenCalledWith("plugin:window|destroy", {
      label: "main",
    });
  });

  it("destroys the window on a second close request while shutdown is pending", async () => {
    let finishClose: (() => void) | undefined;
    const closeWorkbench = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          finishClose = resolve;
        }),
    );
    useWorkbenchStore.setState({ closeWorkbench });
    render(<App />);
    await waitFor(() => expect(closeEventListener).toBeTypeOf("function"));

    act(() => {
      void closeEventListener?.();
    });
    await waitFor(() => expect(closeWorkbench).toHaveBeenCalledOnce());
    await act(async () => {
      await closeEventListener?.();
    });

    expect(invoke).toHaveBeenCalledWith(
      "plugin:window|destroy",
      { label: "main" },
      undefined,
    );
    await act(async () => {
      finishClose?.();
    });
  });

  it("unsubscribes from close requests when App unmounts", async () => {
    const screen = render(<App />);
    await waitFor(() => expect(closeEventListener).toBeTypeOf("function"));

    screen.unmount();

    expect(closeUnlisten).toHaveBeenCalledOnce();
  });
});

describe("App harness selection after bootstrap", () => {
  beforeEach(() => {
    listeners.clear();
    discoverCodex.mockReset();
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {
        metadata: { currentWindow: { label: "main" } },
        transformCallback: (callback: (event: unknown) => void) => {
          const id = ++callbackId;
          callbacks.set(id, callback);
          return id;
        },
        invoke,
      },
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

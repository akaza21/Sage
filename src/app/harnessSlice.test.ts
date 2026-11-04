import { beforeEach, describe, expect, it, vi } from "vitest";

import type { HarnessDefinition } from "../domain/harness";
import type * as AcpDesktopModule from "../services/acpDesktop";
import { useWorkbenchStore } from "./store";

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

const available: HarnessDefinition = {
  ...detecting,
  installState: "available",
  bindingId: "codex-app-server",
};

const simulator: HarnessDefinition = {
  id: "simulator",
  name: "Simulator",
  description: "",
  source: "development",
  transport: "legacySimulator",
  installState: "available",
  bindingId: null,
  accountModelDiscovery: false,
};

const discovery = {
  rootId: "root-a",
  projectId: "project-a",
  projectGeneration: "0",
  bindingId: "codex-app-server",
  account: "signedIn",
  models: [{ id: "gpt-x", name: "GPT-X", isDefault: true }],
};

function seed(overrides: Record<string, unknown> = {}) {
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
    error: null,
    ...overrides,
  });
}

describe("harness discovery reconciliation", () => {
  beforeEach(() => {
    discoverCodex.mockReset();
    seed();
  });

  it("treats a persisted selection that is still detecting as pending, not an error", async () => {
    seed({ selectedHarnessId: "codex" });
    await useWorkbenchStore
      .getState()
      .loadHarnessModelsForProjection("codex", null, 0, "project-a");
    expect(discoverCodex).not.toHaveBeenCalled();
    expect(useWorkbenchStore.getState().harnessDiscoveryPhase).toBe("checking");
    expect(useWorkbenchStore.getState().harnessDiscoveryError).toBeNull();
    expect(useWorkbenchStore.getState().error).toBeNull();
  });

  it("selects the first available real harness once discovery settles", async () => {
    discoverCodex.mockResolvedValue(discovery);
    await useWorkbenchStore
      .getState()
      .applyHarnessDiscovery([simulator, available]);
    const state = useWorkbenchStore.getState();
    expect(state.selectedHarnessId).toBe("codex");
    expect(state.selectedModelId).toBe("gpt-x");
    expect(state.harnessDiscoveryPhase).toBe("ready");
  });

  it("loads models when a restored unavailable selection becomes available", async () => {
    seed({
      harnesses: [{ ...available, installState: "unavailable" }],
      selectedHarnessId: "codex",
    });
    await useWorkbenchStore
      .getState()
      .loadHarnessModelsForProjection("codex", null, 0, "project-a");
    expect(useWorkbenchStore.getState().models).toEqual([]);
    discoverCodex.mockResolvedValue(discovery);
    await useWorkbenchStore.getState().applyHarnessDiscovery([available]);
    const state = useWorkbenchStore.getState();
    expect(state.selectedHarnessId).toBe("codex");
    expect(state.selectedModelId).toBe("gpt-x");
    expect(state.models).toEqual([
      { id: "gpt-x", name: "GPT-X", isDefault: true },
    ]);
  });

  it("drops a discovery result issued for a superseded project generation", async () => {
    let release: () => void = () => undefined;
    discoverCodex.mockImplementation(async () => {
      await new Promise<void>((resolve) => {
        release = resolve;
      });
      return discovery;
    });
    const pending = useWorkbenchStore
      .getState()
      .applyHarnessDiscovery([available]);
    useWorkbenchStore.setState({ projectGeneration: 1 });
    release();
    await pending;
    const state = useWorkbenchStore.getState();
    expect(state.models).toEqual([]);
    expect(state.selectedModelId).toBeNull();
    expect(state.harnessDiscoveryPhase).not.toBe("ready");
  });
});

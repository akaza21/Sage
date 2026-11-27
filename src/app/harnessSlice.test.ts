import { beforeEach, describe, expect, it, vi } from "vitest";

import type { HarnessDefinition } from "../domain/harness";
import type { ModelOption } from "../domain/workbench";
import type * as AcpHarnessDesktopModule from "../services/acpHarnessDesktop";
import { desktop } from "../services/desktop";
import { effectiveDraftChoice } from "./newChatDraftNavigation";
import type { NewChatDraft } from "./newChatDraftSlice";
import { useWorkbenchStore } from "./store";

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

  it("selects the first available harness in catalog order once discovery settles", async () => {
    discoverCodex.mockResolvedValue(discovery);
    await useWorkbenchStore
      .getState()
      .applyHarnessDiscovery([available, simulator]);
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

  it("keeps a still-available user selection ahead of catalog order", async () => {
    seed({ harnesses: [available], selectedHarnessId: available.id });
    discoverCodex.mockResolvedValue(discovery);
    await useWorkbenchStore
      .getState()
      .applyHarnessDiscovery([simulator, available]);
    expect(useWorkbenchStore.getState().selectedHarnessId).toBe(available.id);
    expect(useWorkbenchStore.getState().selectedModelId).toBe("gpt-x");
  });

  it("clears the new-chat selection and models when no harness is available", async () => {
    seed({
      harnesses: [available],
      selectedHarnessId: available.id,
      selectedModelId: "gpt-x",
      models: discovery.models,
    });
    await useWorkbenchStore
      .getState()
      .applyHarnessDiscovery([{ ...available, installState: "unavailable" }]);
    expect(useWorkbenchStore.getState().selectedHarnessId).toBeNull();
    expect(useWorkbenchStore.getState().selectedModelId).toBeNull();
    expect(useWorkbenchStore.getState().models).toEqual([]);
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

describe("startup before discovery and listings settle", () => {
  const pi: HarnessDefinition = {
    ...simulator,
    id: "pi",
    name: "Pi",
    source: "builtIn",
    transport: "acp",
    bindingId: "pi-rpc",
  };
  const sonnet = { id: "sonnet", name: "Sonnet", isDefault: true };
  const opus = { id: "opus", name: "Opus", isDefault: true };
  const draft = { harnessId: "pi", modelId: "sonnet" } as NewChatDraft;
  let answerLive: (models: ModelOption[]) => void = () => undefined;

  beforeEach(() => {
    // As the bootstrap restores it: the harness used last and the listing
    // it gave then.
    seed({
      harnesses: [simulator, pi],
      selectedHarnessId: "pi",
      selectedModelId: "sonnet",
      models: [sonnet],
    });
    vi.spyOn(desktop, "listHarnessModels").mockImplementation(
      () => new Promise((resolve) => (answerLive = resolve)),
    );
  });

  it("shows the remembered models at once but sends only with a model the live listing confirms", async () => {
    void useWorkbenchStore
      .getState()
      .loadHarnessModelsForProjection("pi", "sonnet", 0, "project-a");
    expect(useWorkbenchStore.getState().models).toEqual([sonnet]);
    expect(useWorkbenchStore.getState().selectedModelId).toBe("sonnet");
    expect(effectiveDraftChoice(draft, useWorkbenchStore.getState())).toBe(
      null,
    );
    // The live listing no longer offers the remembered model.
    answerLive([opus]);
    await vi.waitFor(() =>
      expect(useWorkbenchStore.getState().harnessDiscoveryPhase).toBe("ready"),
    );
    expect(useWorkbenchStore.getState().selectedModelId).toBe("opus");
    expect(
      effectiveDraftChoice(draft, useWorkbenchStore.getState())?.modelId,
    ).toBe("opus");
  });

  it("keeps the remembered harness while it is checked, though another answered first", async () => {
    const checking = {
      ...pi,
      installState: "unavailable",
      versionChecking: true,
    };
    await useWorkbenchStore
      .getState()
      .applyHarnessDiscovery([simulator, checking as HarnessDefinition]);
    let state = useWorkbenchStore.getState();
    expect(state.selectedHarnessId).toBe("pi");
    expect(state.models).toEqual([sonnet]);
    expect(state.harnessDiscoveryPhase).toBe("checking");
    void useWorkbenchStore.getState().applyHarnessDiscovery([simulator, pi]);
    answerLive([sonnet]);
    await vi.waitFor(() =>
      expect(useWorkbenchStore.getState().harnessDiscoveryPhase).toBe("ready"),
    );
    state = useWorkbenchStore.getState();
    expect(state.selectedHarnessId).toBe("pi");
    expect(state.selectedModelId).toBe("sonnet");
  });
});

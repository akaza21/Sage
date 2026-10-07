import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { DEFAULT_SESSION_ACCESS_MODE } from "../../domain/sessionAccessMode";
import { decodeSessionConfig } from "../../domain/acpSessionConfig";
import {
  getWorkbenchPorts,
  resetWorkbenchPortsForTests,
} from "../../ports/workbenchRuntime";
import { useSessionAccessModeStore } from "../../app/sessionAccessModeStore";
import { useWorkbenchStore } from "../../app/store";
import {
  harnessModelIdentity,
  rememberHarnessModels,
} from "../../app/harnessModelCache";
import { ChatHarnessControls } from "./ChatHarnessControls";

const noop = () => Promise.resolve();
const initialWorkbenchState = useWorkbenchStore.getState();

function renderControls(overrides: Record<string, unknown> = {}) {
  const renderWith = (props: Record<string, unknown>) => (
    <ChatHarnessControls
      availableBinding
      canPrompt
      discoveryPhase="ready"
      sessionHarnessId={null}
      sessionModelId={null}
      selectedHarnessId={null}
      selectedModelId={null}
      projectName={null}
      turnBusy={false}
      sessionConfig={null}
      permissionRequests={false}
      modePending={false}
      modeError={null}
      focused={false}
      onSetSessionMode={noop}
      onSetSessionConfigOption={noop}
      onSelectHarness={noop}
      onSelectModel={() => {}}
      onModeRemoved={() => {}}
      {...props}
    />
  );
  const view = render(renderWith(overrides));
  return {
    ...view,
    rerenderControls: (props: Record<string, unknown>) =>
      view.rerender(renderWith(props)),
  };
}

beforeEach(async () => {
  vi.restoreAllMocks();
  resetWorkbenchPortsForTests();
  await getWorkbenchPorts().settings.setSessionAccessMode(
    DEFAULT_SESSION_ACCESS_MODE,
  );
  useSessionAccessModeStore.setState({
    mode: DEFAULT_SESSION_ACCESS_MODE,
    ready: true,
    error: null,
  });
});

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialWorkbenchState, true);
});

function openOptions() {
  fireEvent.click(screen.getByRole("button", { name: "Composer options" }));
}

describe("composer permission mode picker", () => {
  it("keeps project choices reachable from the plus menu", () => {
    const switchProject = vi.fn().mockResolvedValue(undefined);
    useWorkbenchStore.setState({
      project: { id: "alpha", name: "Alpha", roots: [] },
      projects: [
        { id: "alpha", name: "Alpha", rootCount: 1, lastOpenedAt: 1 },
        { id: "beta", name: "Beta", rootCount: 1, lastOpenedAt: 2 },
      ],
      switchProject,
    });
    renderControls({ projectName: "Alpha" });
    openOptions();
    fireEvent.click(screen.getByRole("button", { name: "Project" }));
    fireEvent.click(screen.getByRole("option", { name: "Beta" }));
    expect(switchProject).toHaveBeenCalledWith("beta");
  });

  it("blocks project switching while a turn is active", () => {
    useWorkbenchStore.setState({
      project: { id: "alpha", name: "Alpha", roots: [] },
      projects: [
        { id: "alpha", name: "Alpha", rootCount: 1, lastOpenedAt: 1 },
        { id: "beta", name: "Beta", rootCount: 1, lastOpenedAt: 2 },
      ],
    });
    renderControls({ projectName: "Alpha", turnBusy: true });
    expect(
      screen.getByRole("button", { name: "Composer options" }),
    ).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Project" })).toBeNull();
  });

  it("moves focus into the options dialog and restores it on Escape", () => {
    renderControls();
    const trigger = screen.getByRole("button", { name: "Composer options" });
    fireEvent.click(trigger);
    expect(
      screen.getByRole("dialog", { name: "Composer options menu" }),
    ).toBeVisible();
    expect(document.activeElement).toBe(
      screen.getByRole("button", { name: "Permission mode" }),
    );
    fireEvent.keyDown(document, { key: "Escape" });
    expect(
      screen.queryByRole("dialog", { name: "Composer options menu" }),
    ).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });

  it("dismisses the model dialog on outside click and returns focus", () => {
    renderControls();
    const trigger = screen.getByRole("button", { name: "Model and effort" });
    fireEvent.click(trigger);
    expect(
      screen.getByRole("dialog", { name: "Model and effort menu" }),
    ).toBeVisible();
    fireEvent.pointerDown(document.body);
    expect(
      screen.queryByRole("dialog", { name: "Model and effort menu" }),
    ).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });

  it("closes the parent dialog when a nested picker receives Escape", () => {
    renderControls();
    const trigger = screen.getByRole("button", { name: "Composer options" });
    fireEvent.click(trigger);
    fireEvent.click(screen.getByRole("button", { name: "Permission mode" }));
    fireEvent.keyDown(document.activeElement as Element, { key: "Escape" });
    expect(
      screen.queryByRole("dialog", { name: "Composer options menu" }),
    ).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });

  it("shows effective read-only access when ask is unsupported", () => {
    useSessionAccessModeStore.setState({ mode: "ask" });
    renderControls({ permissionRequests: false });
    expect(screen.getByText("Read-only")).toHaveClass("composer__access-label");
    expect(screen.queryByText("Ask each time")).toBeNull();
  });

  it("shows the selected model and effort in one trigger with access beside it", () => {
    const sessionConfig = decodeSessionConfig({
      sageAgentSessionId: "session-1",
      runtimeEpoch: "1",
      configRevision: "1",
      modes: null,
      configOptions: [
        {
          id: "model",
          name: "Model",
          type: "select",
          currentValue: "opus",
          options: [{ value: "opus", name: "Claude Opus 5.5" }],
        },
        {
          id: "effort",
          name: "Effort",
          type: "select",
          currentValue: "high",
          options: [{ value: "high", name: "High" }],
        },
      ],
    });
    if (!sessionConfig) throw new Error("session config did not decode");
    useSessionAccessModeStore.setState({ mode: "workspaceWrite" });
    renderControls({ sessionConfig });
    expect(
      screen.getByRole("button", { name: "Model and effort" }),
    ).toHaveTextContent("Claude Opus 5.5 High");
    expect(screen.getByText("Full access")).toHaveClass(
      "composer__access-label--write",
    );
    fireEvent.click(screen.getByRole("button", { name: "Model and effort" }));
    expect(screen.getByRole("button", { name: "Session model" })).toBeVisible();
    expect(
      screen.getByRole("button", { name: "Session effort" }),
    ).toBeVisible();
  });

  it("keeps the harness visible beside the model and permission choices under plus", () => {
    renderControls();
    expect(
      screen.getByRole("button", { name: "Composer options" }),
    ).toBeVisible();
    expect(screen.getByRole("button", { name: "Harness" })).toBeVisible();
    expect(
      screen.getByRole("button", { name: "Model and effort" }),
    ).toBeVisible();
    expect(
      screen
        .getByRole("button", { name: "Harness" })
        .closest(".composer__control-trigger"),
    ).not.toBeNull();
    expect(
      screen.getByRole("button", { name: "Model and effort" }),
    ).toHaveClass("composer__control-trigger");
    fireEvent.click(screen.getByRole("button", { name: "Composer options" }));
    expect(
      screen.getByRole("button", { name: "Permission mode" }),
    ).toBeVisible();
  });

  it("gives harness and model triggers the same icon and control treatment", () => {
    renderControls();
    const harness = screen.getByRole("button", { name: "Harness" });
    const model = screen.getByRole("button", { name: "Model and effort" });
    expect(harness).toHaveClass("composer__picker-trigger");
    expect(model).toHaveClass("composer__picker-trigger");
    expect(harness.querySelector("svg")).not.toBeNull();
    expect(model.querySelector("svg[data-model-icon]")).not.toBeNull();
  });

  it("does not offer harness changes during an active turn", () => {
    renderControls({ turnBusy: true });
    expect(screen.getByRole("button", { name: "Harness" })).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "Model and effort" }),
    ).toBeDisabled();
  });

  it("closes an already open menu when a turn starts", () => {
    const { rerenderControls } = renderControls();
    openOptions();
    expect(
      screen.getByRole("dialog", { name: "Composer options menu" }),
    ).toBeVisible();
    rerenderControls({ turnBusy: true });
    expect(
      screen.queryByRole("dialog", { name: "Composer options menu" }),
    ).toBeNull();
    expect(
      screen.getByRole("button", { name: "Composer options" }),
    ).toBeDisabled();
  });

  it("shows the selected harness even when a previous session belongs to another", () => {
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "claude",
          name: "Claude Code",
          description: "",
          accountModelDiscovery: false,
          installState: "available",
          source: "builtIn",
          transport: "acp",
          bindingId: "claude-agent-acp",
        },
        {
          id: "codex",
          name: "Codex",
          description: "",
          accountModelDiscovery: false,
          installState: "available",
          source: "builtIn",
          transport: "acp",
          bindingId: "codex-app-server",
        },
      ],
    });
    renderControls({ selectedHarnessId: "codex", sessionHarnessId: "claude" });
    expect(screen.getByRole("button", { name: "Harness" })).toHaveTextContent(
      "Codex",
    );
  });

  it("distinguishes harness choices with their icons", () => {
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "claude",
          name: "Claude Code",
          description: "",
          installState: "available",
          source: "builtIn",
          accountModelDiscovery: false,
          transport: "acp",
          bindingId: "claude-agent-acp",
        },
        {
          id: "codex",
          name: "Codex",
          description: "",
          installState: "available",
          source: "builtIn",
          accountModelDiscovery: false,
          transport: "acp",
          bindingId: "codex-app-server",
        },
        {
          id: "opencode",
          name: "OpenCode",
          description: "",
          installState: "available",
          source: "builtIn",
          accountModelDiscovery: false,
          transport: "acp",
          bindingId: "opencode-acp",
        },
      ],
    });
    renderControls({ selectedHarnessId: "claude" });
    fireEvent.click(screen.getByRole("button", { name: "Harness" }));
    expect(
      screen
        .getByRole("option", { name: /Claude Code/ })
        .querySelector('[data-harness-icon="claude"]'),
    ).not.toBeNull();
    expect(screen.getByText("Harnesses")).toBeVisible();
    expect(
      screen
        .getByRole("option", { name: /Claude Code/ })
        .querySelector(".searchable-picker__selected-check"),
    ).not.toBeNull();
    expect(
      screen
        .getByRole("option", { name: /Codex/ })
        .querySelector('[data-harness-icon="codex"]'),
    ).not.toBeNull();
    expect(
      screen
        .getByRole("option", { name: /OpenCode/ })
        .querySelector('[data-harness-icon="opencode"]'),
    ).not.toBeNull();
  });

  it("shows available harnesses as logo, name and model rows with keyboard selection", () => {
    const claude = {
      id: "claude",
      name: "Claude Code",
      description: "",
      installState: "available" as const,
      source: "builtIn" as const,
      accountModelDiscovery: false,
      transport: "acp" as const,
      bindingId: "claude-agent-acp",
    };
    const codex = {
      ...claude,
      id: "codex",
      name: "Codex",
      bindingId: "codex-app-server",
    };
    const opencode = {
      ...claude,
      id: "opencode",
      name: "OpenCode",
      bindingId: "opencode-acp",
    };
    const unavailable = {
      ...claude,
      id: "missing",
      name: "Missing",
      installState: "notInstalled" as const,
      bindingId: null,
    };
    rememberHarnessModels(harnessModelIdentity(codex), [
      { id: "astra", name: "Astra High", isDefault: true },
    ]);
    useWorkbenchStore.setState({
      harnesses: [claude, codex, opencode, unavailable],
      selectedHarnessId: claude.id,
      models: [{ id: "opus", name: "Opus 5" }],
    });
    const sessionConfig = decodeSessionConfig({
      sageAgentSessionId: "session-1",
      runtimeEpoch: "1",
      configRevision: "1",
      modes: null,
      configOptions: [
        {
          id: "model",
          name: "Model",
          type: "select",
          currentValue: "opus",
          options: [{ value: "opus", name: "Opus 5" }],
        },
        {
          id: "effort",
          name: "Effort",
          type: "select",
          currentValue: "high",
          options: [{ value: "high", name: "High" }],
        },
      ],
    });
    if (!sessionConfig) throw new Error("session config did not decode");
    const onSelectHarness = vi.fn().mockResolvedValue(undefined);
    renderControls({
      selectedHarnessId: claude.id,
      selectedModelId: "opus",
      sessionHarnessId: claude.id,
      sessionModelId: "opus",
      sessionConfig,
      onSelectHarness,
    });
    const trigger = screen.getByRole("button", { name: "Harness" });
    expect(trigger).toHaveTextContent("Claude Code");
    expect(trigger).toHaveTextContent("Opus 5 High");
    expect(
      trigger.querySelector('img[src="/harness-claude.svg"]'),
    ).not.toBeNull();
    fireEvent.click(trigger);
    const claudeRow = screen.getByRole("option", { name: /Claude Code/ });
    const codexRow = screen.getByRole("option", { name: /Codex/ });
    expect(claudeRow).toHaveTextContent("Opus 5 High");
    expect(
      claudeRow.querySelector(".searchable-picker__selected-check"),
    ).not.toBeNull();
    expect(codexRow).toHaveTextContent("Astra High");
    expect(
      codexRow.querySelector('img[src="/harness-codex.svg"]'),
    ).not.toBeNull();
    expect(screen.getByRole("option", { name: /OpenCode/ })).toHaveTextContent(
      "Default model",
    );
    expect(screen.getByRole("option", { name: /Missing/ })).toBeDisabled();
    const input = screen.getByRole("combobox", { name: "Harness" });
    expect(input).toHaveAttribute("readonly");
    fireEvent.keyDown(input, { key: "ArrowDown" });
    fireEvent.keyDown(input, { key: "ArrowDown" });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onSelectHarness).toHaveBeenCalledWith("codex");
  });

  it("uses the same popover surface for harness and model choices", () => {
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "codex",
          name: "Codex",
          description: "",
          installState: "available",
          source: "builtIn",
          accountModelDiscovery: false,
          transport: "acp",
          bindingId: "codex-app-server",
        },
      ],
    });
    renderControls({ selectedHarnessId: "codex" });
    fireEvent.click(screen.getByRole("button", { name: "Harness" }));
    expect(screen.getByRole("listbox").closest(".menu-popover")).toHaveClass(
      "composer__popover",
    );
    fireEvent.keyDown(document, { key: "Escape" });
    fireEvent.click(screen.getByRole("button", { name: "Model and effort" }));
    expect(
      screen.getByRole("dialog", { name: "Model and effort menu" }),
    ).toHaveClass("composer__popover");
  });

  it("does not show a previous harness session model after selection changes", () => {
    const sessionConfig = decodeSessionConfig({
      sageAgentSessionId: "session-1",
      runtimeEpoch: "1",
      configRevision: "1",
      modes: null,
      configOptions: [
        {
          id: "model",
          name: "Model",
          type: "select",
          currentValue: "claude-model",
          options: [{ value: "claude-model", name: "Claude Opus" }],
        },
      ],
    });
    if (!sessionConfig) throw new Error("session config did not decode");
    useWorkbenchStore.setState({
      selectedHarnessId: "codex",
      models: [{ id: "codex-model", name: "Codex 6" }],
      harnesses: [
        {
          id: "codex",
          name: "Codex",
          description: "Codex",
          installState: "available",
          source: "builtIn",
          accountModelDiscovery: false,
          transport: "acp",
          bindingId: "codex-app-server",
        },
      ],
    });
    renderControls({
      selectedHarnessId: "codex",
      selectedModelId: "codex-model",
      sessionHarnessId: "claude",
      sessionConfig,
    });
    expect(
      screen.getByRole("button", { name: "Model and effort" }),
    ).toHaveTextContent("Codex 6");
    fireEvent.click(screen.getByRole("button", { name: "Model and effort" }));
    expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
      "Codex 6",
    );
    expect(screen.queryByRole("button", { name: "Session model" })).toBeNull();
  });

  it("does not show session model or effort after selecting another model in the same harness", () => {
    const sessionConfig = decodeSessionConfig({
      sageAgentSessionId: "session-1",
      runtimeEpoch: "1",
      configRevision: "1",
      modes: null,
      configOptions: [
        {
          id: "model",
          name: "Model",
          type: "select",
          currentValue: "old",
          options: [{ value: "old", name: "Old Model" }],
        },
        {
          id: "effort",
          name: "Effort",
          type: "select",
          currentValue: "high",
          options: [{ value: "high", name: "High" }],
        },
      ],
    });
    if (!sessionConfig) throw new Error("session config did not decode");
    useWorkbenchStore.setState({
      selectedHarnessId: "codex",
      models: [{ id: "new", name: "New Model" }],
      harnesses: [
        {
          id: "codex",
          name: "Codex",
          description: "",
          installState: "available",
          source: "builtIn",
          accountModelDiscovery: false,
          transport: "acp",
          bindingId: "codex-app-server",
        },
      ],
    });
    renderControls({
      selectedHarnessId: "codex",
      selectedModelId: "new",
      sessionHarnessId: "codex",
      sessionModelId: "old",
      sessionConfig,
    });
    expect(
      screen.getByRole("button", { name: "Model and effort" }),
    ).toHaveTextContent("New Model");
    expect(
      screen.getByRole("button", { name: "Model and effort" }),
    ).not.toHaveTextContent("Old Model");
    expect(
      screen.getByRole("button", { name: "Model and effort" }),
    ).not.toHaveTextContent("High");
    fireEvent.click(screen.getByRole("button", { name: "Model and effort" }));
    expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
      "New Model",
    );
    expect(screen.queryByText("Old Model")).toBeNull();
    expect(screen.queryByText("High")).toBeNull();
    expect(screen.queryByRole("button", { name: "Session model" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Session effort" })).toBeNull();
  });

  it("replaces the hard-coded chip with the live persisted mode", () => {
    renderControls();
    openOptions();
    expect(screen.queryByText("Read-only policy")).toBeNull();
    expect(
      screen.getByRole("button", { name: "Permission mode" }),
    ).toHaveTextContent("Read-only");
  });

  it("reflects a different persisted mode", () => {
    useSessionAccessModeStore.setState({ mode: "workspaceWrite" });
    renderControls();
    openOptions();
    expect(
      screen.getByRole("button", { name: "Permission mode" }),
    ).toHaveTextContent("Auto-accept edits in workspace");
  });

  it("persists a chosen mode through the settings port", async () => {
    const save = vi.spyOn(getWorkbenchPorts().settings, "setSessionAccessMode");
    renderControls();
    openOptions();
    fireEvent.click(screen.getByRole("button", { name: "Permission mode" }));
    fireEvent.click(
      screen.getByRole("option", { name: /Auto-accept edits in workspace/ }),
    );
    expect(save).toHaveBeenCalledWith("workspaceWrite");
    await waitFor(() =>
      expect(useSessionAccessModeStore.getState().mode).toBe("workspaceWrite"),
    );
  });

  it("states when the chosen mode takes effect", () => {
    renderControls();
    openOptions();
    fireEvent.click(screen.getByRole("button", { name: "Permission mode" }));
    expect(
      screen.getByText("Applies from your next message in any chat."),
    ).toBeInTheDocument();
  });

  it("gates ask honestly while permission prompts are unconfirmed", () => {
    renderControls();
    openOptions();
    fireEvent.click(screen.getByRole("button", { name: "Permission mode" }));
    const ask = screen.getByRole("option", { name: /Ask each time/ });
    expect(ask).toBeDisabled();
    expect(ask).toHaveTextContent(
      "This harness has not confirmed support for permission prompts.",
    );
  });

  it("admits ask when the session projection confirms permission requests", () => {
    renderControls({ permissionRequests: true });
    openOptions();
    fireEvent.click(screen.getByRole("button", { name: "Permission mode" }));
    const ask = screen.getByRole("option", { name: /Ask each time/ });
    expect(ask).toBeEnabled();
    expect(ask).not.toHaveTextContent(
      "This harness has not confirmed support for permission prompts.",
    );
  });

  it("hides the picker without an available binding", () => {
    renderControls({ availableBinding: false });
    openOptions();
    expect(
      screen.queryByRole("button", { name: "Permission mode" }),
    ).toBeNull();
  });

  it("is not actionable before the persisted mode has been restored", () => {
    useSessionAccessModeStore.setState({ ready: false });
    renderControls();
    openOptions();
    expect(
      screen.getByRole("button", { name: "Permission mode" }),
    ).toBeDisabled();
  });

  it("surfaces a save failure instead of silently reverting", async () => {
    vi.spyOn(
      getWorkbenchPorts().settings,
      "setSessionAccessMode",
    ).mockRejectedValue(new Error("nope"));
    renderControls();
    openOptions();
    fireEvent.click(screen.getByRole("button", { name: "Permission mode" }));
    fireEvent.click(
      screen.getByRole("option", { name: /Auto-accept edits in workspace/ }),
    );

    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(
        "Permission mode could not be saved.",
      ),
    );
  });
});

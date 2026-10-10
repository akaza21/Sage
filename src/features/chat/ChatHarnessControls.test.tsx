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
      modelLabelScope="project/chat/session-1"
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

function confirmedModelConfig() {
  return decodeSessionConfig({
    sageAgentSessionId: "session-1",
    runtimeEpoch: "1",
    configRevision: "1",
    modes: null,
    configOptions: [
      {
        id: "model",
        name: "Model",
        type: "select",
        currentValue: "session-model",
        options: [{ value: "session-model", name: "Confirmed session model" }],
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
}

describe("last confirmed model label", () => {
  const selection = {
    selectedHarnessId: "harness-a",
    sessionHarnessId: "harness-a",
    selectedModelId: "launch-model",
    sessionModelId: "launch-model",
    modelLabelScope: "project/chat/session-1",
  };

  it.each(["null", "removed"] as const)(
    "keeps a session-selected label after Stop and %s config without restoring options",
    (removal) => {
      useWorkbenchStore.setState({
        selectedHarnessId: "harness-a",
        models: [],
      });
      const sessionConfig = confirmedModelConfig();
      if (!sessionConfig) throw new Error("Config did not decode");
      const view = renderControls({
        ...selection,
        turnBusy: true,
        sessionConfig,
      });
      expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
        "Confirmed session model",
      );
      expect(
        screen.getByRole("button", { name: "Thinking level" }),
      ).toBeDisabled();
      view.rerenderControls({
        ...selection,
        sessionHarnessId: null,
        turnBusy: false,
        sessionConfig:
          removal === "null"
            ? null
            : { ...sessionConfig, removed: true, configOptions: [] },
      });
      const chip = screen.getByRole("button", { name: "Model" });
      expect(chip).toHaveTextContent("Confirmed session model");
      expect(chip).toBeDisabled();
      fireEvent.click(chip);
      expect(screen.queryByRole("dialog", { name: "Model picker" })).toBeNull();
      expect(
        screen.queryByRole("button", { name: "Thinking level" }),
      ).toBeNull();
    },
  );

  it.each([
    { modelLabelScope: "project/other-chat/session-1" },
    { modelLabelScope: "project/chat/session-2" },
    { modelLabelScope: "other-project/chat/session-1" },
    { selectedHarnessId: "harness-b", sessionHarnessId: "harness-b" },
    { selectedModelId: "other-selection" },
    { sessionModelId: "other-session-selection" },
  ])(
    "invalidates a confirmed label on ownership or selection change %j",
    (change) => {
      useWorkbenchStore.setState({
        selectedHarnessId: "harness-a",
        models: [],
      });
      const view = renderControls({
        ...selection,
        sessionConfig: confirmedModelConfig(),
      });
      expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
        "Confirmed session model",
      );
      view.rerenderControls({ ...selection, ...change, sessionConfig: null });
      expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
        /^Model$/,
      );
      view.rerenderControls({ ...selection, sessionConfig: null });
      expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
        /^Model$/,
      );
    },
  );

  it("prefers current config and the confirmed model's catalog name over cached or launch labels", () => {
    useWorkbenchStore.setState({
      selectedHarnessId: "harness-a",
      models: [{ id: "launch-model", name: "Launch model" }],
    });
    const sessionConfig = confirmedModelConfig();
    if (!sessionConfig) throw new Error("Config did not decode");
    const view = renderControls({ ...selection, sessionConfig });
    view.rerenderControls({ ...selection, sessionConfig: null });
    expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
      "Confirmed session model",
    );
    useWorkbenchStore.setState({
      models: [{ id: "session-model", name: "Updated catalog label" }],
    });
    view.rerenderControls({ ...selection, sessionConfig: null });
    expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
      "Updated catalog label",
    );
    view.rerenderControls({
      ...selection,
      sessionConfig: {
        ...sessionConfig,
        configOptions: [
          {
            ...sessionConfig.configOptions[0],
            type: "select",
            currentValue: "new-model",
            options: [{ value: "new-model", name: "New confirmed model" }],
          },
        ],
      },
    });
    expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
      "New confirmed model",
    );
  });

  it("invalidates the label when the authoritative Project changes with the same Chat/session scope", () => {
    useWorkbenchStore.setState({
      selectedHarnessId: "harness-a",
      models: [],
      project: { id: "project-one", name: "One", roots: [] },
    });
    const view = renderControls({
      ...selection,
      sessionConfig: confirmedModelConfig(),
    });
    useWorkbenchStore.setState({
      project: { id: "project-two", name: "Two", roots: [] },
    });
    view.rerenderControls({ ...selection, sessionConfig: null });
    expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
      /^Model$/,
    );
  });

  it("keeps two side-by-side pane labels isolated when one config expires or changes owner", () => {
    useWorkbenchStore.setState({ selectedHarnessId: "harness-a", models: [] });
    const config = confirmedModelConfig();
    if (!config) throw new Error("Config did not decode");
    const first = renderControls({ ...selection, sessionConfig: config });
    const second = renderControls({
      ...selection,
      modelLabelScope: "project/chat-two/session-two",
      sessionConfig: {
        ...config,
        sageAgentSessionId: "session-two",
        configOptions: [
          {
            id: "model",
            name: "Model",
            type: "select",
            currentValue: "other-model",
            options: [{ value: "other-model", name: "Other pane model" }],
          },
        ],
      },
    });
    first.rerenderControls({ ...selection, sessionConfig: null });
    expect(
      screen
        .getAllByRole("button", { name: "Model" })
        .map((chip) => chip.textContent),
    ).toEqual(["Confirmed session model", "Other pane model"]);
    second.rerenderControls({
      ...selection,
      modelLabelScope: "project/chat-three/session-three",
      sessionConfig: null,
    });
    expect(
      screen
        .getAllByRole("button", { name: "Model" })
        .map((chip) => chip.textContent),
    ).toEqual(["Confirmed session model", "Model"]);
  });
});

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
    useWorkbenchStore.setState({
      selectedHarnessId: "codex",
      models: [{ id: "astra", name: "Astra" }],
    });
    renderControls({ selectedHarnessId: "codex" });
    const trigger = screen.getByRole("button", { name: "Model" });
    fireEvent.click(trigger);
    expect(screen.getByRole("dialog", { name: "Model picker" })).toBeVisible();
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole("dialog", { name: "Model picker" })).toBeNull();
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

  it("shows separate model and thinking controls with access beside them", () => {
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
    expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
      "Claude Opus 5.5",
    );
    expect(
      screen.getByRole("button", { name: "Thinking level" }),
    ).toHaveTextContent("High");
    expect(screen.getByText("Full access")).toHaveClass(
      "composer__access-label--write",
    );
    fireEvent.click(screen.getByRole("button", { name: "Model" }));
    expect(screen.getByRole("dialog", { name: "Model picker" })).toBeVisible();
  });

  it("sets the selected session model and thinking through their own pickers", () => {
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
          options: [
            { value: "opus", name: "Opus" },
            { value: "sonnet", name: "Sonnet" },
          ],
        },
        {
          id: "effort",
          name: "Effort",
          type: "select",
          currentValue: "medium",
          options: [
            { value: "medium", name: "Medium" },
            { value: "high", name: "High" },
          ],
        },
      ],
    });
    if (!sessionConfig) throw new Error("session config did not decode");
    const onSetSessionConfigOption = vi.fn().mockResolvedValue(undefined);
    const onSelectModel = vi.fn();
    renderControls({
      selectedHarnessId: "claude",
      selectedModelId: "opus",
      sessionHarnessId: "claude",
      sessionModelId: "opus",
      sessionConfig,
      onSetSessionConfigOption,
      onSelectModel,
    });
    fireEvent.click(screen.getByRole("button", { name: "Model" }));
    fireEvent.click(screen.getByRole("gridcell", { name: /Sonnet/ }));
    expect(onSetSessionConfigOption).toHaveBeenCalledWith("model", {
      value: "sonnet",
    });
    expect(onSelectModel).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Thinking level" }));
    fireEvent.click(screen.getByRole("option", { name: "High" }));
    expect(onSetSessionConfigOption).toHaveBeenCalledWith("effort", {
      value: "high",
    });
  });

  it("keeps the harness visible beside the model and permission choices under plus", () => {
    renderControls();
    expect(
      screen.getByRole("button", { name: "Composer options" }),
    ).toBeVisible();
    expect(screen.getByRole("button", { name: "Harness" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Model" })).toBeVisible();
    expect(
      screen
        .getByRole("button", { name: "Harness" })
        .closest(".composer__control-trigger"),
    ).not.toBeNull();
    expect(screen.getByRole("button", { name: "Model" })).toHaveClass(
      "composer__control-trigger",
    );
    fireEvent.click(screen.getByRole("button", { name: "Composer options" }));
    expect(
      screen.getByRole("button", { name: "Permission mode" }),
    ).toBeVisible();
  });

  it("gives harness and model triggers the same control treatment", () => {
    renderControls();
    const harness = screen.getByRole("button", { name: "Harness" });
    const model = screen.getByRole("button", { name: "Model" });
    expect(harness).toHaveClass("composer__picker-trigger");
    expect(model).toHaveClass("composer__picker-trigger");
    expect(harness.querySelector("svg")).not.toBeNull();
    expect(model.querySelector("svg")).not.toBeNull();
  });

  it("does not offer harness changes during an active turn", () => {
    renderControls({ turnBusy: true });
    expect(screen.getByRole("button", { name: "Harness" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Model" })).toBeDisabled();
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
      version: "0.146.0",
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
      harnesses: [unavailable, claude, codex, opencode],
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
    expect(trigger).not.toHaveTextContent("Opus 5 High");
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
      codexRow.querySelector(".searchable-picker__option-copy"),
    ).not.toHaveTextContent("0.146.0");
    expect(codexRow).not.toHaveAccessibleName(/0\.146\.0/);
    expect(
      codexRow.querySelector('img[src="/harness-codex.svg"]'),
    ).not.toBeNull();
    expect(screen.getByRole("option", { name: /OpenCode/ })).toHaveTextContent(
      "Default model",
    );
    expect(screen.getByRole("option", { name: /Missing/ })).toBeDisabled();
    expect(screen.getByRole("option", { name: /Missing/ })).toHaveTextContent(
      "not installed",
    );
    expect(screen.getAllByRole("option").at(-1)).toHaveTextContent("Missing");
    const input = screen.getByRole("combobox", { name: "Harness" });
    expect(input).toHaveAttribute("readonly");
    fireEvent.keyDown(input, { key: "ArrowDown" });
    fireEvent.keyDown(input, { key: "ArrowDown" });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onSelectHarness).toHaveBeenCalledWith("codex");
  });

  it("searches models, keeps favorites, and leaves thinking separate", () => {
    useWorkbenchStore.setState({
      selectedHarnessId: "codex",
      models: [
        { id: "astra", name: "Astra High", contextWindow: 1_000_000 },
        { id: "luna", name: "Luna Fast", contextWindow: 272_000 },
      ],
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
    const onSelectModel = vi.fn();
    renderControls({
      selectedHarnessId: "codex",
      selectedModelId: "astra",
      onSelectModel,
    });
    expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
      "Astra High",
    );
    expect(screen.getByText("1M context")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Model" }));
    const search = screen.getByRole("combobox", { name: "Search models" });
    const grid = screen.getByRole("grid", { name: "Models" });
    expect(search).toHaveAttribute("aria-controls", grid.id);
    fireEvent.change(search, { target: { value: "luna" } });
    expect(screen.queryByRole("gridcell", { name: /Astra High/ })).toBeNull();
    const luna = screen.getByRole("gridcell", { name: /Luna Fast/ });
    expect(luna).toHaveTextContent("Codex");
    fireEvent.keyDown(search, { key: "ArrowDown" });
    expect(search).toHaveAttribute("aria-activedescendant", luna.id);
    expect(grid).toContainElement(luna);
    const favorite = screen.getByRole("button", {
      name: "Favorite Luna Fast",
    });
    expect(favorite.closest('[role="gridcell"]')).not.toBe(luna);
    favorite.focus();
    expect(document.activeElement).toBe(favorite);
    fireEvent.click(favorite);
    fireEvent.change(search, { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: "Favorites" }));
    expect(screen.getByRole("gridcell", { name: /Luna Fast/ })).toBeVisible();
    expect(screen.queryByRole("gridcell", { name: /Astra High/ })).toBeNull();
    fireEvent.click(screen.getByRole("gridcell", { name: /Luna Fast/ }));
    expect(onSelectModel).toHaveBeenCalledWith("luna");
  });

  it("uses the same popover surface for harness and model choices", () => {
    useWorkbenchStore.setState({
      selectedHarnessId: "codex",
      models: [{ id: "astra", name: "Astra" }],
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
    fireEvent.click(screen.getByRole("button", { name: "Model" }));
    expect(screen.getByRole("dialog", { name: "Model picker" })).toHaveClass(
      "composer__popover",
    );
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
    expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
      "Codex 6",
    );
    fireEvent.click(screen.getByRole("button", { name: "Model" }));
    expect(screen.getByRole("gridcell", { name: /Codex 6/ })).toBeVisible();
    expect(screen.queryByRole("gridcell", { name: /Claude Opus/ })).toBeNull();
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
    expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
      "New Model",
    );
    expect(screen.getByRole("button", { name: "Model" })).not.toHaveTextContent(
      "Old Model",
    );
    expect(screen.getByRole("button", { name: "Model" })).not.toHaveTextContent(
      "High",
    );
    fireEvent.click(screen.getByRole("button", { name: "Model" }));
    expect(screen.getByRole("gridcell", { name: /New Model/ })).toBeVisible();
    expect(screen.queryByText("Old Model")).toBeNull();
    expect(screen.queryByText("High")).toBeNull();
    expect(screen.queryByRole("button", { name: "Thinking level" })).toBeNull();
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

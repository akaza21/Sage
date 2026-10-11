import {
  act,
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
  it("shows confirmed effort after catalog failure without inventing effort choices", () => {
    useWorkbenchStore.setState({ selectedHarnessId: "codex", models: [] });
    const sessionConfig = decodeSessionConfig({
      sageAgentSessionId: "session-1",
      runtimeEpoch: "1",
      configRevision: "1",
      modes: null,
      configOptions: [],
      confirmedModel: { id: "actual" },
      confirmedEffort: "high",
    });
    renderControls({
      selectedHarnessId: "codex",
      sessionHarnessId: "codex",
      sessionConfig,
    });
    const trigger = screen.getByRole("button", { name: "Model" });
    expect(trigger).toHaveTextContent(/^actual High$/);
    expect(trigger).toHaveAccessibleDescription("actual High");
    expect(trigger).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Thinking level" })).toBeNull();
  });

  it("does not confirm a requested catalog model until the harness reports identity", () => {
    useWorkbenchStore.setState({
      selectedHarnessId: "codex",
      models: [
        { id: "requested", name: "Requested model" },
        { id: "actual", name: "Actual model" },
      ],
    });
    const selection = {
      selectedHarnessId: "codex",
      sessionHarnessId: "codex",
      selectedModelId: "requested",
      sessionModelId: "requested",
    };
    const view = renderControls(selection);
    const trigger = screen.getByRole("button", { name: "Model" });
    expect(trigger).toHaveTextContent(/^Model$/);
    fireEvent.click(trigger);
    expect(
      screen.getByRole("option", { name: "Requested model" }),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("option", { name: "Requested model" }));
    expect(trigger).toHaveTextContent(/^Model$/);
    view.rerenderControls({
      ...selection,
      sessionConfig: decodeSessionConfig({
        sageAgentSessionId: "session-1",
        runtimeEpoch: "1",
        configRevision: "1",
        modes: null,
        configOptions: [],
        confirmedModel: { id: "actual" },
      }),
    });
    expect(trigger).toHaveTextContent(/^Actual model$/);
    view.rerenderControls({ ...selection, selectedModelId: "actual" });
    expect(trigger).toHaveTextContent(/^Model$/);
  });

  it("retains the matching catalog name while id-only session identity survives catalog removal", () => {
    useWorkbenchStore.setState({
      selectedHarnessId: "codex",
      models: [{ id: "actual", name: "Actual model" }],
    });
    renderControls({
      selectedHarnessId: "codex",
      sessionHarnessId: "codex",
      sessionConfig: decodeSessionConfig({
        sageAgentSessionId: "session-1",
        runtimeEpoch: "1",
        configRevision: "1",
        modes: null,
        configOptions: [],
        confirmedModel: { id: "actual" },
      }),
    });
    const trigger = screen.getByRole("button", { name: "Model" });
    expect(trigger).toHaveTextContent(/^Actual model$/);
    act(() => useWorkbenchStore.setState({ models: [] }));
    expect(trigger).toHaveTextContent(/^Actual model$/);
    expect(trigger).toBeDisabled();
  });

  it("uses the confirmed model identifier when its advertised display name is empty", () => {
    const sessionConfig = confirmedModelConfig();
    if (!sessionConfig) throw new Error("Config did not decode");
    sessionConfig.configOptions[0] = {
      id: "model",
      name: "Model",
      type: "select",
      currentValue: "session-model",
      options: [{ value: "session-model", name: "" }],
    };
    renderControls({ sessionConfig });
    const trigger = screen.getByRole("button", { name: "Model" });
    expect(trigger).toHaveTextContent(/^session-model High$/);
    expect(trigger).toHaveAccessibleDescription("session-model High");
    fireEvent.click(trigger);
    expect(
      screen.getByRole("option", { name: "session-model" }),
    ).toHaveAttribute("aria-selected", "true");
  });

  it("announces the full confirmed model and effort from the model control", () => {
    renderControls({ sessionConfig: confirmedModelConfig() });
    const trigger = screen.getByRole("button", { name: "Model" });
    expect(trigger).toHaveAccessibleDescription("Confirmed session model High");
    expect(trigger).toHaveAttribute("title", "Confirmed session model High");
  });

  it("does not replace an unknown selection with the first or default catalog model", () => {
    useWorkbenchStore.setState({
      selectedHarnessId: "codex",
      models: [
        { id: "first", name: "First model", isDefault: true },
        { id: "second", name: "Second model" },
      ],
    });
    renderControls({ selectedHarnessId: "codex", selectedModelId: "unknown" });
    expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
      /^Model$/,
    );
    fireEvent.click(screen.getByRole("button", { name: "Model" }));
    expect(
      screen
        .getAllByRole("option")
        .filter((option) => option.getAttribute("aria-selected") === "true"),
    ).toHaveLength(0);
  });

  it("retains only emitted identity across config removal and rejects stale owner metadata", () => {
    useWorkbenchStore.setState({ selectedHarnessId: "codex", models: [] });
    const selection = { selectedHarnessId: "codex", sessionHarnessId: "codex" };
    const config = decodeSessionConfig({
      sageAgentSessionId: "session-1",
      runtimeEpoch: "1",
      configRevision: "1",
      modes: null,
      configOptions: [],
      confirmedModel: { id: "actual" },
    });
    if (!config) throw new Error("Config did not decode");
    const view = renderControls({ ...selection, sessionConfig: config });
    expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
      /^actual$/,
    );
    view.rerenderControls({
      ...selection,
      sessionConfig: {
        ...config,
        confirmedModel: { id: "actual", name: "Actual model" },
      },
    });
    expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
      /^Actual model$/,
    );
    view.rerenderControls({ ...selection, sessionConfig: null });
    expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
      /^Actual model$/,
    );
    view.rerenderControls({
      ...selection,
      selectedHarnessId: "other",
      sessionConfig: config,
    });
    expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
      /^Model$/,
    );
  });

  it("shows confirmed non-selectable session identity and effort without a catalog", () => {
    useWorkbenchStore.setState({ selectedHarnessId: "codex", models: [] });
    const sessionConfig = decodeSessionConfig({
      sageAgentSessionId: "session-1",
      runtimeEpoch: "1",
      configRevision: "1",
      modes: null,
      confirmedModel: { id: "gpt-6-astra", name: "GPT-6 Astra" },
      configOptions: [
        {
          id: "effort",
          name: "Effort",
          type: "select",
          currentValue: "high",
          options: [{ value: "high", name: "High" }],
        },
      ],
    });
    renderControls({
      selectedHarnessId: "codex",
      sessionHarnessId: "codex",
      sessionConfig,
    });
    const trigger = screen.getByRole("button", { name: "Model" });
    expect(trigger).toHaveTextContent(/^GPT-6 Astra High$/);
    expect(trigger).toBeDisabled();
    fireEvent.click(trigger);
    expect(screen.queryByRole("listbox", { name: "Select model" })).toBeNull();
  });

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
      expect(
        screen.queryByRole("listbox", { name: "Select model" }),
      ).toBeNull();
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
  it("keeps keyboard selection reachable when an open model catalog is replaced", () => {
    useWorkbenchStore.setState({
      selectedHarnessId: "codex",
      models: [{ id: "old", name: "Old model" }],
    });
    const onSelectModel = vi.fn();
    renderControls({ selectedHarnessId: "codex", onSelectModel });
    fireEvent.click(screen.getByRole("button", { name: "Model" }));
    expect(screen.getByRole("option", { name: "Old model" })).toHaveFocus();
    act(() =>
      useWorkbenchStore.setState({
        models: [{ id: "new", name: "New model" }],
      }),
    );
    expect(screen.queryByRole("option", { name: "Old model" })).toBeNull();
    const next = screen.getByRole("option", { name: "New model" });
    expect(next).toHaveFocus();
    fireEvent.click(next);
    expect(onSelectModel).toHaveBeenCalledExactlyOnceWith("new");
  });

  it("lists an advertised Default without fabrication and bounds model options at 200", () => {
    useWorkbenchStore.setState({
      selectedHarnessId: "codex",
      models: [
        { id: "actual-default", name: "Default" },
        ...Array.from({ length: 200 }, (_, index) => ({
          id: `model-${index + 1}`,
          name: `Advertised model ${index + 1}`,
        })),
      ],
    });
    const onSelectModel = vi.fn();
    renderControls({ selectedHarnessId: "codex", onSelectModel });
    const trigger = screen.getByRole("button", { name: "Model" });
    fireEvent.click(trigger);
    expect(screen.getAllByRole("option")).toHaveLength(200);
    fireEvent.click(screen.getByRole("option", { name: "Default" }));
    expect(onSelectModel).toHaveBeenCalledExactlyOnceWith("actual-default");
    fireEvent.click(trigger);
    fireEvent.keyDown(screen.getByRole("option", { name: "Default" }), {
      key: "End",
    });
    expect(
      screen.getByRole("option", { name: "Advertised model 199" }),
    ).toHaveFocus();
    expect(
      screen.queryByRole("option", { name: "Advertised model 200" }),
    ).toBeNull();
    expect(screen.getByRole("status")).toHaveTextContent(
      "Showing first 200 models",
    );
    expect(
      screen.getByRole("listbox", { name: "Select model" }),
    ).not.toContainElement(screen.getByRole("status"));
  });

  it("dismisses the model list on Tab, restores its trigger and leaves default handling enabled", () => {
    useWorkbenchStore.setState({
      selectedHarnessId: "codex",
      models: [{ id: "actual", name: "Actual model" }],
    });
    renderControls({ selectedHarnessId: "codex", selectedModelId: "actual" });
    const trigger = screen.getByRole("button", { name: "Model" });
    fireEvent.click(trigger);
    const option = screen.getByRole("option", { name: "Actual model" });
    expect(option).toHaveFocus();
    expect(fireEvent.keyDown(option, { key: "Tab" })).toBe(true);
    expect(screen.queryByRole("listbox", { name: "Select model" })).toBeNull();
    expect(trigger).toHaveFocus();
  });

  it("closes a model list when its catalog disappears and does not reopen it on recovery", () => {
    const models = [{ id: "actual", name: "Actual model" }];
    useWorkbenchStore.setState({ selectedHarnessId: "codex", models });
    renderControls({ selectedHarnessId: "codex", selectedModelId: "actual" });
    const trigger = screen.getByRole("button", { name: "Model" });
    fireEvent.click(trigger);
    expect(screen.getByRole("listbox", { name: "Select model" })).toBeVisible();
    act(() => useWorkbenchStore.setState({ models: [] }));
    expect(screen.queryByRole("listbox", { name: "Select model" })).toBeNull();
    act(() => useWorkbenchStore.setState({ models }));
    expect(screen.queryByRole("listbox", { name: "Select model" })).toBeNull();
  });

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
    fireEvent.keyDown(document.activeElement as Element, { key: "Escape" });
    expect(
      screen.queryByRole("dialog", { name: "Composer options menu" }),
    ).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });

  it("releases restored Model trigger focus on pointer down and refocuses the selected model on reopening", () => {
    useWorkbenchStore.setState({
      selectedHarnessId: "codex",
      models: [
        { id: "astra", name: "Astra" },
        { id: "luna", name: "Luna" },
      ],
    });
    renderControls({ selectedHarnessId: "codex", selectedModelId: "luna" });
    const trigger = screen.getByRole("button", { name: "Model" });
    trigger.focus();
    fireEvent.click(trigger, { detail: 0 });
    const selected = screen.getByRole("option", { name: "Luna" });
    expect(selected).toHaveFocus();
    fireEvent.keyDown(selected, { key: "Escape" });
    expect(trigger).toHaveFocus();
    fireEvent.pointerDown(trigger, { pointerType: "mouse" });
    expect(trigger).not.toHaveFocus();
    trigger.focus();
    fireEvent.click(trigger, { detail: 1 });
    expect(screen.getByRole("option", { name: "Luna" })).toHaveFocus();
    fireEvent.keyDown(document.activeElement!, { key: "Home" });
    expect(screen.getByRole("option", { name: "Astra" })).toHaveFocus();
    fireEvent.keyDown(document.activeElement!, { key: "ArrowDown" });
    expect(screen.getByRole("option", { name: "Luna" })).toHaveFocus();
  });

  it("dismisses the model list on outside pointer without stealing focus", () => {
    useWorkbenchStore.setState({
      selectedHarnessId: "codex",
      models: [{ id: "astra", name: "Astra" }],
    });
    renderControls({ selectedHarnessId: "codex" });
    const trigger = screen.getByRole("button", { name: "Model" });
    fireEvent.click(trigger);
    expect(screen.getByRole("listbox", { name: "Select model" })).toBeVisible();
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole("listbox", { name: "Select model" })).toBeNull();
    expect(document.activeElement).toBe(document.body);
  });

  it("closes the nested picker before its parent dialog on successive Escape presses", () => {
    renderControls();
    const trigger = screen.getByRole("button", { name: "Composer options" });
    fireEvent.click(trigger);
    fireEvent.click(screen.getByRole("button", { name: "Permission mode" }));
    fireEvent.keyDown(document.activeElement as Element, { key: "Escape" });
    expect(
      screen.queryByRole("listbox", { name: "Permission mode" }),
    ).toBeNull();
    expect(
      screen.getByRole("dialog", { name: "Composer options menu" }),
    ).toBeVisible();
    expect(
      screen.getByRole("button", { name: "Permission mode" }),
    ).toHaveFocus();
    fireEvent.keyDown(document.activeElement as Element, { key: "Escape" });
    expect(
      screen.queryByRole("dialog", { name: "Composer options menu" }),
    ).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });

  it("shows effective read-only access when ask is unsupported", () => {
    useSessionAccessModeStore.setState({ mode: "ask" });
    renderControls({ permissionRequests: false });
    expect(screen.getByRole("button", { name: "Access options" })).toHaveClass(
      "composer__access-label",
    );
    expect(
      screen.getByRole("button", { name: "Access options" }),
    ).toHaveTextContent("Read-only");
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
    expect(
      screen.getByRole("button", { name: "Access options" }),
    ).toHaveTextContent("Full access");
    expect(screen.getByRole("button", { name: "Access options" })).toHaveClass(
      "composer__access-label--write",
    );
    fireEvent.click(screen.getByRole("button", { name: "Model" }));
    expect(screen.getByRole("listbox", { name: "Select model" })).toBeVisible();
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
    fireEvent.click(screen.getByRole("option", { name: /Sonnet/ }));
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

  it("shows harness names without model or installation subtitles and keeps reasons accessible", () => {
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
    expect(
      claudeRow.querySelector(".searchable-picker__option-copy"),
    ).toHaveTextContent(/^Claude Code$/);
    expect(
      claudeRow.querySelector(".searchable-picker__selected-check"),
    ).not.toBeNull();
    expect(
      codexRow.querySelector(".searchable-picker__option-copy"),
    ).toHaveTextContent(/^Codex$/);
    expect(
      codexRow.querySelector(".searchable-picker__option-copy"),
    ).not.toHaveTextContent("0.146.0");
    expect(codexRow).not.toHaveAccessibleName(/0\.146\.0/);
    expect(
      codexRow.querySelector('img[src="/harness-codex.svg"]'),
    ).not.toBeNull();
    expect(screen.getByRole("option", { name: /OpenCode/ })).toHaveTextContent(
      /^OpenCode$/,
    );
    expect(screen.getByRole("option", { name: /Missing/ })).toBeDisabled();
    expect(
      screen
        .getByRole("option", { name: /Missing/ })
        .querySelector(".searchable-picker__option-copy"),
    ).toHaveTextContent(/^Missing$/);
    expect(
      screen.getByRole("option", { name: /Missing/ }),
    ).toHaveAccessibleDescription("Harness is not installed.");
    expect(screen.getAllByRole("option").at(-1)).toHaveTextContent("Missing");
    expect(screen.queryByRole("combobox", { name: "Harness" })).toBeNull();
    expect(claudeRow).toHaveFocus();
    fireEvent.keyDown(claudeRow, { key: "ArrowDown" });
    expect(codexRow).toHaveFocus();
    fireEvent.keyDown(codexRow, { key: "Enter" });
    fireEvent.click(codexRow);
    expect(onSelectHarness).toHaveBeenCalledWith("codex");
  });

  it("shows a plain Select model list with a selected check and keyboard typeahead", () => {
    useWorkbenchStore.setState({
      selectedHarnessId: "codex",
      models: [
        { id: "astra", name: "Astra", contextWindow: 1_000_000 },
        { id: "luna", name: "Luna", contextWindow: 272_000 },
      ],
    });
    const onSelectModel = vi.fn();
    renderControls({
      selectedHarnessId: "codex",
      selectedModelId: "astra",
      onSelectModel,
    });
    expect(screen.getByText("1M context")).toBeVisible();
    const trigger = screen.getByRole("button", { name: "Model" });
    fireEvent.click(trigger);
    const list = screen.getByRole("listbox", { name: "Select model" });
    expect(trigger).toHaveAttribute("aria-haspopup", "listbox");
    expect(trigger).toHaveAttribute("aria-controls", list.id);
    const title = screen.getByText("Select model");
    expect(title).toBeVisible();
    expect(list).not.toContainElement(title);
    expect(list).toHaveAttribute("aria-labelledby", title.id);
    expect(list).not.toHaveAttribute("aria-label");
    expect(list.closest(".menu-popover")).not.toHaveAttribute("aria-label");
    expect(Array.from(list.children)).toEqual(screen.getAllByRole("option"));
    expect(
      screen.queryByRole("combobox", { name: "Search models" }),
    ).toBeNull();
    expect(screen.queryByRole("button", { name: /Favorite/ })).toBeNull();
    expect(screen.queryByRole("option", { name: "Default" })).toBeNull();
    const astra = screen.getByRole("option", { name: "Astra" });
    const luna = screen.getByRole("option", { name: "Luna" });
    expect(astra).toHaveAttribute("aria-selected", "true");
    expect(
      astra.querySelector(".searchable-picker__selected-check"),
    ).not.toBeNull();
    expect(luna).toHaveAttribute("aria-selected", "false");
    expect(astra.textContent).toBe("Astra");
    expect(luna.textContent).toBe("Luna");
    expect(astra).toHaveFocus();
    fireEvent.keyDown(astra, { key: "l" });
    expect(luna).toHaveFocus();
    fireEvent.click(luna);
    expect(onSelectModel).toHaveBeenCalledExactlyOnceWith("luna");
    expect(screen.queryByRole("listbox", { name: "Select model" })).toBeNull();
    expect(trigger).toHaveFocus();
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
    expect(
      screen
        .getByRole("listbox", { name: "Select model" })
        .closest(".menu-popover"),
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
    expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
      /^Model$/,
    );
    fireEvent.click(screen.getByRole("button", { name: "Model" }));
    expect(screen.getByRole("option", { name: /Codex 6/ })).toBeVisible();
    expect(screen.queryByRole("option", { name: /Claude Opus/ })).toBeNull();
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
      /^Model$/,
    );
    expect(screen.getByRole("button", { name: "Model" })).not.toHaveTextContent(
      "Old Model",
    );
    expect(screen.getByRole("button", { name: "Model" })).not.toHaveTextContent(
      "High",
    );
    fireEvent.click(screen.getByRole("button", { name: "Model" }));
    expect(screen.getByRole("option", { name: /New Model/ })).toBeVisible();
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

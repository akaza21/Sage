import {
  act,
  cleanup,
  within,
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
const nativeAccessModes = [
  {
    id: "readOnly" as const,
    label: "Read-only",
    description: "Read workspace files; no writes or network.",
    fallback: true,
  },
  {
    id: "workspaceWrite" as const,
    label: "Workspace edits",
    description:
      "Edits and commands run without asking inside the workspace and temp folders; no network access.",
  },
];

function renderControls(overrides: Record<string, unknown> = {}) {
  const renderWith = (props: Record<string, unknown>) => (
    <ChatHarnessControls
      availableBinding
      canPrompt
      discoveryPhase="ready"
      modelLabelScope="project/chat/session-1"
      sessionHarnessId="access-test"
      sessionModelId={null}
      selectedHarnessId="access-test"
      selectedModelId={null}
      projectName={null}
      turnBusy={false}
      sessionConfig={null}
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
  useWorkbenchStore.setState({
    harnesses: [
      {
        id: "access-test",
        name: "Native policy fixture",
        description: "",
        installState: "available",
        source: "builtIn",
        transport: "acp",
        bindingId: "fixture",
        accountModelDiscovery: false,
        accessModes: nativeAccessModes,
      },
    ],
  });
  await getWorkbenchPorts().settings.setSessionAccessModes({
    fixture: DEFAULT_SESSION_ACCESS_MODE,
  });
  useSessionAccessModeStore.setState({
    modes: { fixture: DEFAULT_SESSION_ACCESS_MODE },
    ready: true,
    error: null,
  });
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  useWorkbenchStore.setState(initialWorkbenchState, true);
});

it("keeps Harness and Access mutually exclusive without pointer events", async () => {
  renderControls();
  const access = screen.getByRole("button", { name: "Access options" });
  const harness = screen.getByRole("button", { name: "Harness" });
  fireEvent.click(access);
  fireEvent.click(harness);
  expect(screen.queryByRole("menu", { name: "Access mode" })).toBeNull();
  expect(screen.getByRole("listbox")).toBeInTheDocument();
  fireEvent.click(access);
  expect(screen.queryByRole("listbox")).toBeNull();
  fireEvent.keyDown(await screen.findByRole("menu", { name: "Access mode" }), {
    key: "Escape",
  });
  expect(access).toHaveFocus();
});

function openOptions() {
  fireEvent.click(screen.getByRole("button", { name: "Composer options" }));
}

it("shows the selected harness, human model, effort availability and access before a first turn", async () => {
  useWorkbenchStore.setState({
    selectedHarnessId: "access-test",
    models: [
      { id: "claude-sonnet-4-5-20250929", name: "Claude Sonnet 4 5 20250929" },
    ],
  });
  const { rerenderControls } = renderControls({
    sessionHarnessId: null,
    selectedModelId: "claude-sonnet-4-5-20250929",
  });
  expect(screen.getByRole("button", { name: "Harness" })).toBeVisible();
  expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
    "Claude Sonnet 4.5",
  );
  expect(screen.getByRole("button", { name: "Access options" })).toBeVisible();
  const effort = screen.getByRole("button", { name: "Thinking level" });
  expect(effort).toBeDisabled();
  expect(effort).toHaveAccessibleDescription(
    "Available when the harness advertises effort controls for this session.",
  );
  rerenderControls({
    sessionHarnessId: "access-test",
    sessionConfig: confirmedModelConfig(),
  });
  expect(
    screen.getByRole("button", { name: "Thinking level: High" }),
  ).toBeEnabled();
  expect(screen.queryByRole("button", { name: "Thinking level" })).toBeNull();
});

it("keeps advertised model groups and exact IDs when changing session models", async () => {
  const onSetSessionConfigOption = vi.fn();
  renderControls({
    onSetSessionConfigOption,
    sessionConfig: decodeSessionConfig({
      sageAgentSessionId: "session-1",
      runtimeEpoch: "1",
      configRevision: "1",
      modes: null,
      configOptions: [
        {
          id: "model",
          name: "Model",
          category: "model",
          type: "select",
          currentValue: "claude-sonnet-4-5-20250929",
          options: [
            {
              group: "default",
              name: "Recommended by harness",
              options: [
                {
                  value: "claude-sonnet-4-5-20250929",
                  name: "Claude Sonnet 4 5 20250929",
                },
              ],
            },
            {
              group: "other",
              name: "Other choices",
              options: [{ value: "private-id", name: "Private model" }],
            },
          ],
        },
      ],
    }),
  });
  expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
    "Claude Sonnet 4.5",
  );
  fireEvent.click(screen.getByRole("button", { name: "Model" }));
  const group = await screen.findByRole("group", { name: "Other choices" });
  fireEvent.click(within(group).getByRole("option", { name: "Private model" }));
  expect(onSetSessionConfigOption).toHaveBeenCalledExactlyOnceWith("model", {
    value: "private-id",
  });
});

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
  it("shows Pi's wire-confirmed model and effort after interrupt without inventing a reset default", async () => {
    const onSetSessionConfigOption = vi.fn();
    const view = renderControls({ onSetSessionConfigOption });
    expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
      /^Model$/,
    );
    expect(
      screen.queryByRole("button", { description: "Select effort" }),
    ).toBeNull();
    const sessionConfig = decodeSessionConfig({
      sageAgentSessionId: "session-1",
      runtimeEpoch: "1",
      configRevision: "1",
      modes: null,
      configOptions: [
        {
          id: "pi.model",
          name: "Model",
          category: "model",
          type: "select",
          currentValue: "openai:fixture-model",
          options: [
            { value: "openai:fixture-model", name: "Synthetic replay" },
            {
              value: "openai:fixture-model-two",
              name: "Synthetic alternative",
            },
          ],
        },
        {
          id: "pi.thinking",
          name: "Thinking",
          category: "effort",
          type: "select",
          currentValue: "medium",
          options: ["off", "low", "medium", "high"].map((value) => ({
            value,
            name: value,
          })),
        },
      ],
    });
    view.rerenderControls({
      sessionConfig,
      turnBusy: true,
      onSetSessionConfigOption,
    });
    expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
      /^Synthetic replay$/,
    );
    expect(
      screen.getByRole("button", { name: "Thinking level: medium" }),
    ).toBeDisabled();
    view.rerenderControls({
      sessionConfig,
      turnBusy: false,
      onSetSessionConfigOption,
    });
    const effort = screen.getByRole("button", {
      name: "Thinking level: medium",
    });
    expect(effort).toBeEnabled();
    fireEvent.click(effort);
    expect(await screen.findByRole("slider")).toHaveAttribute(
      "aria-valuetext",
      "medium",
    );
    expect(screen.queryByRole("button", { name: "Reset effort" })).toBeNull();
    expect(onSetSessionConfigOption).not.toHaveBeenCalled();
  });

  it("omits Pi effort when its binding publishes model-only config after unusable or absent levels", async () => {
    const sessionConfig = decodeSessionConfig({
      sageAgentSessionId: "session-1",
      runtimeEpoch: "1",
      configRevision: "1",
      modes: null,
      configOptions: [
        {
          id: "pi.model",
          name: "Model",
          category: "model",
          type: "select",
          currentValue: "openai:fixture-model",
          options: [
            { value: "openai:fixture-model", name: "Synthetic replay" },
          ],
        },
      ],
    });
    const view = renderControls({ sessionConfig, turnBusy: true });
    view.rerenderControls({ sessionConfig, turnBusy: false });
    expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
      /^Synthetic replay$/,
    );
    expect(
      screen.queryByRole("button", { description: "Select effort" }),
    ).toBeNull();
    expect(screen.queryByRole("slider")).toBeNull();
    expect(screen.queryByRole("button", { name: "Reset effort" })).toBeNull();
  });
  it.each(["Codex", "Claude Code", "Pi", "OpenCode", "Oh My Pi"])(
    "keeps %s confirmed idle identity, picker subtitle, effort after interrupt and advertised reset",
    async (name) => {
      useWorkbenchStore.setState({
        harnesses: [
          {
            id: "access-test",
            name,
            description: "",
            installState: "available",
            source: "builtIn",
            transport: "acp",
            bindingId: "fixture",
            accountModelDiscovery: false,
          },
        ],
      });
      const config = confirmedModelConfig()!;
      config.configOptions[1] = {
        id: "thinking",
        category: "effort",
        name: "Thinking",
        type: "select",
        currentValue: "high",
        defaultValue: "low",
        options: [
          { value: "low", name: "Low" },
          { value: "high", name: "High" },
        ],
      };
      const view = renderControls({ sessionConfig: config });
      expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
        /^Confirmed session model$/,
      );
      fireEvent.click(screen.getByRole("button", { name: "Harness" }));
      const row = screen.getByRole("option", { name: `${name} — available` });
      expect(row).toHaveTextContent(`${name}Confirmed session model High`);
      expect(
        row.querySelector(".searchable-picker__subtitle"),
      ).toHaveTextContent("Confirmed session model High");
      fireEvent.keyDown(row, { key: "Escape" });
      view.rerenderControls({ sessionConfig: config, turnBusy: true });
      expect(
        screen.getByRole("button", { name: "Thinking level: High" }),
      ).toBeDisabled();
      view.rerenderControls({ sessionConfig: config, turnBusy: false });
      const effort = screen.getByRole("button", {
        name: "Thinking level: High",
      });
      expect(effort).toBeEnabled();
      fireEvent.click(effort);
      expect(
        await screen.findByRole("button", { name: "Reset effort" }),
      ).toBeVisible();
      expect(
        await screen.findByRole("button", { name: "Reset effort" }),
      ).toBeEnabled();
    },
  );

  it("shows effort once when a separate effort control is available", async () => {
    renderControls({ sessionConfig: confirmedModelConfig() });
    expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
      /^Confirmed session model$/,
    );
    expect(screen.getAllByText("High", { exact: true })).toHaveLength(1);
    expect(
      screen.getByRole("button", { description: "Select effort" }),
    ).toHaveTextContent("High");
  });

  it("keeps effort reachable through Add when its compact toolbar control yields", async () => {
    useWorkbenchStore.setState({
      project: { id: "a", name: "A", roots: [] },
      projects: [{ id: "a", name: "A", rootCount: 0, lastOpenedAt: 1 }],
    });
    const observer = vi.fn();
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(callback: (entries: unknown[]) => void) {
          observer.mockImplementation(callback);
        }
        observe() {
          observer([{ contentRect: { width: 280 } }]);
        }
        disconnect() {}
      },
    );
    const host = document.createElement("form");
    host.className = "composer";
    document.body.append(host);
    const view = render(
      <ChatHarnessControls
        availableBinding
        canPrompt
        discoveryPhase="ready"
        modelLabelScope="chat"
        sessionHarnessId="access-test"
        sessionModelId={null}
        selectedHarnessId="access-test"
        selectedModelId={null}
        projectName="A"
        turnBusy={false}
        sessionConfig={confirmedModelConfig()}
        modePending={false}
        modeError={null}
        focused
        onSetSessionMode={noop}
        onSetSessionConfigOption={noop}
        onSelectHarness={noop}
        onSelectModel={() => {}}
        onModeRemoved={() => {}}
      />,
      { container: host },
    );
    fireEvent.click(screen.getByRole("button", { name: "Composer options" }));
    const project = screen.getByRole("menuitem", { name: "Project: A" });
    expect(
      host.querySelector(".composer__controls > .project-scope"),
    ).toBeNull();
    fireEvent.click(project);
    const projects = screen.getByRole("combobox", { name: "Project: A" });
    const candidate = screen.getByRole("option", { name: /^A$/ });
    fireEvent.pointerDown(candidate);
    expect(projects).toBeVisible();
    fireEvent.keyDown(projects, { key: "Escape" });
    expect(project).toHaveFocus();
    fireEvent.click(screen.getByRole("menuitem", { name: "Thinking level" }));
    expect(
      screen.getByRole("dialog", { name: "Thinking level" }),
    ).toBeVisible();
    expect(await screen.findByRole("slider")).toHaveAttribute(
      "aria-valuetext",
      "High",
    );
    expect(screen.queryByRole("menu")).toBeNull();
    fireEvent.keyDown(screen.getByRole("dialog", { name: "Thinking level" }), {
      key: "Escape",
    });
    expect(
      screen.getByRole("button", { name: "Composer options" }),
    ).toHaveFocus();
    view.unmount();
    host.remove();
    vi.unstubAllGlobals();
  });

  it("does not subtitle a draft catalog selection as a confirmed model", async () => {
    useWorkbenchStore.setState({
      selectedHarnessId: "access-test",
      models: [{ id: "draft", name: "Draft model" }],
    });
    renderControls({ sessionHarnessId: null, selectedModelId: "draft" });
    fireEvent.click(screen.getByRole("button", { name: "Harness" }));
    expect(
      within(screen.getByRole("listbox")).queryByText("Draft model"),
    ).toBeNull();
  });

  it.each([null, "previous-harness"])(
    "shows a known draft catalog selection apart from session %s",
    (sessionHarnessId) => {
      useWorkbenchStore.setState({
        selectedHarnessId: "access-test",
        models: [{ id: "chosen", name: "Chosen model" }],
      });
      renderControls({
        sessionHarnessId,
        selectedModelId: "chosen",
      });
      expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
        /^Chosen model$/,
      );
    },
  );

  it("does not offer inert effort actions when prompting is unavailable", async () => {
    renderControls({ sessionConfig: confirmedModelConfig(), canPrompt: false });
    expect(
      screen.getByRole("button", { name: "Composer options" }),
    ).toBeDisabled();
  });

  it("shows only the selected harness's confirmed model and effort in its picker row", async () => {
    renderControls({ sessionConfig: confirmedModelConfig() });
    fireEvent.click(screen.getByRole("button", { name: "Harness" }));
    expect(screen.getByRole("option")).toHaveTextContent(
      "Confirmed session model High",
    );
    expect(
      screen.getByRole("button", { name: "Harness" }),
    ).not.toHaveTextContent("Confirmed session model");
  });

  it.each([false, true])(
    "prefers explicit effort over thought_level regardless of order (%s)",
    async (reverse) => {
      const onSetSessionConfigOption = vi.fn().mockResolvedValue(undefined);
      const options = [
        {
          id: "legacy",
          category: "thought_level",
          name: "Thinking",
          type: "select",
          currentValue: "low",
          options: [
            { value: "low", name: "Legacy low" },
            { value: "high", name: "Legacy high" },
          ],
        },
        {
          id: "explicit",
          category: "effort",
          name: "Effort",
          type: "select",
          currentValue: "medium",
          options: [
            { value: "medium", name: "Medium" },
            { value: "max", name: "Maximum" },
          ],
        },
      ];
      renderControls({
        sessionConfig: decodeSessionConfig({
          sageAgentSessionId: "session-1",
          runtimeEpoch: "1",
          configRevision: "1",
          modes: null,
          configOptions: reverse ? [...options].reverse() : options,
        }),
        onSetSessionConfigOption,
      });
      fireEvent.click(
        screen.getByRole("button", { description: "Select effort" }),
      );
      const slider = await screen.findByRole("slider");
      expect(slider).toHaveAttribute("aria-valuetext", "Medium");
      fireEvent.keyDown(slider, { key: "End" });
      expect(onSetSessionConfigOption).toHaveBeenCalledExactlyOnceWith(
        "explicit",
        { value: "max" },
      );
    },
  );
  it.each([true, false])(
    "uses generic thought_level without rewriting its category (present: %s)",
    async (categorized) => {
      const config = confirmedModelConfig()!;
      if (categorized) config.configOptions[1].category = "thought_level";
      else config.configOptions[1].id = "thought_level";
      renderControls({ sessionConfig: config });
      fireEvent.click(
        screen.getByRole("button", { description: "Select effort" }),
      );
      expect(await screen.findByRole("slider")).toHaveAttribute(
        "aria-valuetext",
        "High",
      );
      expect(config.configOptions[1].category).toBe(
        categorized ? "thought_level" : undefined,
      );
    },
  );
  it("shows confirmed effort after catalog failure without inventing effort choices", async () => {
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
    expect(
      screen.queryByRole("button", { description: "Select effort" }),
    ).toBeNull();
  });

  it("does not confirm a requested catalog model until the harness reports identity", async () => {
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

  it("retains the matching catalog name while id-only session identity survives catalog removal", async () => {
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

  it("uses the confirmed model identifier when its advertised display name is empty", async () => {
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
    expect(trigger).toHaveTextContent(/^session-model$/);
    expect(trigger).toHaveAccessibleDescription("session-model High");
    fireEvent.click(trigger);
    expect(
      await screen.findByRole("option", { name: "session-model" }),
    ).toHaveAttribute("aria-selected", "true");
  });

  it("announces the full confirmed model and effort from the model control", async () => {
    renderControls({ sessionConfig: confirmedModelConfig() });
    const trigger = screen.getByRole("button", { name: "Model" });
    expect(trigger).toHaveAccessibleDescription("Confirmed session model High");
    expect(trigger).toHaveAttribute("title", "Confirmed session model High");
  });

  it("does not replace an unknown selection with the first or default catalog model", async () => {
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
    await screen.findByRole("listbox", { name: "Select model" });
    expect(
      screen
        .getAllByRole("option")
        .filter((option) => option.getAttribute("aria-selected") === "true"),
    ).toHaveLength(0);
  });

  it("retains only emitted identity across config removal and rejects stale owner metadata", async () => {
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

  it("shows confirmed non-selectable session identity and effort without a catalog", async () => {
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
    expect(trigger).toHaveTextContent(/^GPT-6 Astra$/);
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
        screen.getByRole("button", { description: "Select effort" }),
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
        screen.queryByRole("button", { description: "Select effort" }),
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

  it("prefers current config and the confirmed model's catalog name over cached or launch labels", async () => {
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

  it("invalidates the label when the authoritative Project changes with the same Chat/session scope", async () => {
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

  it("keeps two side-by-side pane labels isolated when one config expires or changes owner", async () => {
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
  it("keeps keyboard selection reachable when an open model catalog is replaced", async () => {
    useWorkbenchStore.setState({
      selectedHarnessId: "codex",
      models: [{ id: "old", name: "Old model" }],
    });
    const onSelectModel = vi.fn();
    renderControls({ selectedHarnessId: "codex", onSelectModel });
    fireEvent.click(screen.getByRole("button", { name: "Model" }));
    expect(
      await screen.findByRole("option", { name: "Old model" }),
    ).toHaveFocus();
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

  it("lists an advertised Default without fabrication and bounds model options at 200", async () => {
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
    await screen.findByRole("listbox", { name: "Select model" });
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

  it("dismisses the model list on Tab and preserves its restored trigger focus", async () => {
    useWorkbenchStore.setState({
      selectedHarnessId: "codex",
      models: [{ id: "actual", name: "Actual model" }],
    });
    renderControls({ selectedHarnessId: "codex", selectedModelId: "actual" });
    const trigger = screen.getByRole("button", { name: "Model" });
    fireEvent.click(trigger);
    const option = await screen.findByRole("option", { name: "Actual model" });
    expect(option).toHaveFocus();
    expect(fireEvent.keyDown(option, { key: "Tab" })).toBe(false);
    expect(screen.queryByRole("listbox", { name: "Select model" })).toBeNull();
    expect(trigger).toHaveFocus();
  });

  it("closes a model list when its catalog disappears and does not reopen it on recovery", async () => {
    const models = [{ id: "actual", name: "Actual model" }];
    useWorkbenchStore.setState({ selectedHarnessId: "codex", models });
    renderControls({ selectedHarnessId: "codex", selectedModelId: "actual" });
    const trigger = screen.getByRole("button", { name: "Model" });
    fireEvent.click(trigger);
    expect(
      await screen.findByRole("listbox", { name: "Select model" }),
    ).toBeVisible();
    act(() => useWorkbenchStore.setState({ models: [] }));
    expect(screen.queryByRole("listbox", { name: "Select model" })).toBeNull();
    act(() => useWorkbenchStore.setState({ models }));
    expect(screen.queryByRole("listbox", { name: "Select model" })).toBeNull();
  });

  it("keeps the named project icon inside its clickable target and switches projects", async () => {
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
    const project = screen.getByRole("button", { name: "Project: Alpha" });
    expect(project).toHaveAttribute("title", "Project: Alpha");
    expect(project.querySelector(".project-scope__icon")).toHaveAttribute(
      "aria-hidden",
      "true",
    );
    fireEvent.click(project);
    fireEvent.click(screen.getByRole("option", { name: "Beta" }));
    expect(switchProject).toHaveBeenCalledWith("beta");
  });

  it("blocks project switching while a turn is active", async () => {
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
    expect(
      screen.getByRole("button", { name: "Project: Alpha" }),
    ).toBeDisabled();
  });

  it("moves focus into the action menu and restores it on Escape", async () => {
    useWorkbenchStore.setState({
      projects: [{ id: "alpha", name: "Alpha", rootCount: 1, lastOpenedAt: 1 }],
    });
    renderControls({ imageAction: { disabled: false, onChoose: vi.fn() } });
    const trigger = screen.getByRole("button", { name: "Composer options" });
    fireEvent.click(trigger);
    expect(
      screen.getByRole("menu", { name: "Composer options menu" }),
    ).toBeVisible();
    expect(document.activeElement).toBe(
      screen.getByRole("menuitem", { name: "Add images" }),
    );
    fireEvent.keyDown(document.activeElement as Element, { key: "Escape" });
    expect(
      screen.queryByRole("menu", { name: "Composer options menu" }),
    ).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });

  it.each(["Composer options", "Access options"])(
    "opens only one surface from %s and returns Escape focus to the current opener",
    async (name) => {
      renderControls({
        projectName: "Alpha",
        imageAction: { disabled: false, onChoose: vi.fn() },
      });
      const opener = screen.getByRole("button", { name });
      const other = screen.getByRole("button", {
        name:
          name === "Composer options" ? "Access options" : "Composer options",
      });
      fireEvent.click(opener);
      expect(opener).toHaveAttribute("aria-expanded", "true");
      expect(other).toHaveAttribute("aria-expanded", "false");
      fireEvent.click(other);
      expect(opener).toHaveAttribute("aria-expanded", "false");
      expect(other).toHaveAttribute("aria-expanded", "true");
      await screen.findByRole("menu");
      expect([
        ...screen.queryAllByRole("menu"),
        ...screen.queryAllByRole("dialog"),
      ]).toHaveLength(1);
      fireEvent.keyDown(
        name === "Composer options"
          ? screen.getByRole("menu", { name: "Access mode" })
          : screen.getByRole("menu", { name: "Composer options menu" }),
        { key: "Escape" },
      );
      expect(other).toHaveFocus();
      expect(other).toHaveAttribute("aria-expanded", "false");
    },
  );

  it.each([false, true])(
    "returns action-menu Tab focus to its opener (reverse=%s)",
    (shiftKey) => {
      useWorkbenchStore.setState({
        projects: [
          { id: "alpha", name: "Alpha", rootCount: 1, lastOpenedAt: 1 },
        ],
      });
      renderControls({ imageAction: { disabled: false, onChoose: vi.fn() } });
      const trigger = screen.getByRole("button", { name: "Composer options" });
      fireEvent.click(trigger);
      const control = screen.getByRole("menuitem", { name: "Add images" });
      expect(control).toHaveFocus();
      fireEvent.keyDown(control, { key: "Tab", shiftKey });
      expect(
        screen.queryByRole("menu", { name: "Composer options menu" }),
      ).toBeNull();
      expect(trigger).toHaveFocus();
    },
  );

  it("releases restored Model trigger focus on pointer down and refocuses the selected model on reopening", async () => {
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
    const selected = await screen.findByRole("option", { name: "Luna" });
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

  it("dismisses the model list on outside pointer without stealing focus", async () => {
    useWorkbenchStore.setState({
      selectedHarnessId: "codex",
      models: [{ id: "astra", name: "Astra" }],
    });
    renderControls({ selectedHarnessId: "codex" });
    const trigger = screen.getByRole("button", { name: "Model" });
    fireEvent.click(trigger);
    expect(
      await screen.findByRole("listbox", { name: "Select model" }),
    ).toBeVisible();
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole("listbox", { name: "Select model" })).toBeNull();
    expect(document.activeElement).toBe(document.body);
  });

  it("replaces Add with the project picker and closes it on a single Escape", async () => {
    useWorkbenchStore.setState({
      projects: [{ id: "alpha", name: "Alpha", rootCount: 1, lastOpenedAt: 1 }],
    });
    renderControls({ projectName: "Alpha" });
    const trigger = screen.getByRole("button", { name: "Composer options" });
    fireEvent.click(trigger);
    fireEvent.click(screen.getByRole("button", { name: "Project: Alpha" }));
    fireEvent.keyDown(document.activeElement as Element, { key: "Escape" });
    expect(
      screen.queryByRole("listbox", { name: "Project: Alpha" }),
    ).toBeNull();
    expect(
      screen.queryByRole("menu", { name: "Composer options menu" }),
    ).toBeNull();
    expect(
      screen.getByRole("button", { name: "Project: Alpha" }),
    ).toHaveFocus();
    expect(trigger).toHaveAttribute("aria-expanded", "false");
  });

  it("shows effective read-only access when ask is unsupported", async () => {
    useSessionAccessModeStore.setState({ modes: { fixture: "ask" } });
    renderControls();
    expect(screen.getByRole("button", { name: "Access options" })).toHaveClass(
      "composer__access-label",
    );
    expect(
      screen.getByRole("button", { name: "Access options" }),
    ).toHaveTextContent("Read-only");
    expect(screen.queryByText("Ask each time")).toBeNull();
  });

  it("shows separate model and thinking controls with access beside them", async () => {
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
    useSessionAccessModeStore.setState({
      modes: { fixture: "workspaceWrite" },
    });
    renderControls({ sessionConfig });
    expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
      "Claude Opus 5.5",
    );
    expect(
      screen.getByRole("button", { description: "Select effort" }),
    ).toHaveTextContent("High");
    expect(
      screen.getByRole("button", { name: "Access options" }),
    ).toHaveTextContent("Workspace edits");
    expect(screen.getByRole("button", { name: "Access options" })).toHaveClass(
      "composer__access-label--write",
    );
    fireEvent.click(screen.getByRole("button", { name: "Model" }));
    expect(
      await screen.findByRole("listbox", { name: "Select model" }),
    ).toBeVisible();
  });

  it("sets the selected session model and thinking through their own pickers", async () => {
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
    fireEvent.click(await screen.findByRole("option", { name: /Sonnet/ }));
    expect(onSetSessionConfigOption).toHaveBeenCalledWith("model", {
      value: "sonnet",
    });
    expect(onSelectModel).not.toHaveBeenCalled();
    fireEvent.click(
      screen.getByRole("button", { description: "Select effort" }),
    );
    fireEvent.keyDown(
      await screen.findByRole("slider", { name: "Thinking level" }),
      {
        key: "End",
      },
    );
    expect(onSetSessionConfigOption).toHaveBeenCalledWith("effort", {
      value: "high",
    });
  });

  it("keeps access directly beside the harness and model, not nested under plus", async () => {
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
      screen.getByRole("button", { name: "Access options" }),
    ).toBeVisible();
    expect(
      screen.queryByRole("button", { name: "Permission mode" }),
    ).toBeNull();
    expect(
      screen
        .getByRole("button", { name: "Access options" })
        .closest('[role="dialog"]'),
    ).toBeNull();
  });

  it("gives harness and model triggers the same control treatment", async () => {
    renderControls();
    const harness = screen.getByRole("button", { name: "Harness" });
    const model = screen.getByRole("button", { name: "Model" });
    expect(harness).toHaveClass("composer__picker-trigger");
    expect(model).toHaveClass("composer__picker-trigger");
    expect(harness.querySelector("svg")).not.toBeNull();
    expect(model.querySelector("svg")).not.toBeNull();
  });

  it("does not offer harness changes during an active turn", async () => {
    renderControls({ turnBusy: true });
    expect(screen.getByRole("button", { name: "Harness" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Model" })).toBeDisabled();
  });

  it("closes an already open menu when a turn starts", async () => {
    const { rerenderControls } = renderControls({
      imageAction: { disabled: false, onChoose: vi.fn() },
    });
    openOptions();
    expect(
      screen.getByRole("menu", { name: "Composer options menu" }),
    ).toBeVisible();
    rerenderControls({ turnBusy: true });
    expect(
      screen.queryByRole("menu", { name: "Composer options menu" }),
    ).toBeNull();
    expect(
      screen.getByRole("button", { name: "Composer options" }),
    ).toBeDisabled();
  });

  it("shows the selected harness even when a previous session belongs to another", async () => {
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

  it("distinguishes harness choices with their icons", async () => {
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

  it("shows confirmed model subtitles without installation details and keeps reasons accessible", async () => {
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
    ).toHaveTextContent(/^Claude CodeOpus 5 High$/);
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

  it("shows a plain Select model list with a selected check and keyboard typeahead", async () => {
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
    const list = await screen.findByRole("listbox", { name: "Select model" });
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

  it("uses the same popover surface for harness and model choices", async () => {
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
      (await screen.findByRole("listbox", { name: "Select model" })).closest(
        ".menu-popover",
      ),
    ).toHaveClass("composer__popover");
  });

  it("does not show a previous harness session model after selection changes", async () => {
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
      /^Codex 6$/,
    );
    expect(screen.getByRole("button", { name: "Model" })).not.toHaveTextContent(
      "Claude Opus",
    );
    fireEvent.click(screen.getByRole("button", { name: "Model" }));
    expect(
      await screen.findByRole("option", { name: /Codex 6/ }),
    ).toBeVisible();
    expect(screen.queryByRole("option", { name: /Claude Opus/ })).toBeNull();
  });

  it("does not show session model or effort after selecting another model in the same harness", async () => {
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
    expect(
      await screen.findByRole("option", { name: /New Model/ }),
    ).toBeVisible();
    expect(screen.queryByText("Old Model")).toBeNull();
    expect(screen.queryByText("High")).toBeNull();
    expect(
      screen.queryByRole("button", { description: "Select effort" }),
    ).toBeNull();
  });

  it("replaces the hard-coded chip with the live persisted mode", async () => {
    renderControls();
    openOptions();
    expect(screen.queryByText("Read-only policy")).toBeNull();
    expect(
      screen.getByRole("button", { name: "Access options" }),
    ).toHaveTextContent("Read-only");
  });

  it("reflects a different persisted mode", async () => {
    useSessionAccessModeStore.setState({
      modes: { fixture: "workspaceWrite" },
    });
    renderControls();
    openOptions();
    expect(
      screen.getByRole("button", { name: "Access options" }),
    ).toHaveTextContent("Workspace edits");
  });

  it("persists a chosen mode through the settings port", async () => {
    const save = vi.spyOn(
      getWorkbenchPorts().settings,
      "setSessionAccessModes",
    );
    renderControls();
    openOptions();
    fireEvent.click(screen.getByRole("button", { name: "Access options" }));
    fireEvent.click(
      await screen.findByRole("menuitemradio", { name: "Workspace edits" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Switch access" }));
    expect(save).toHaveBeenCalledWith({ fixture: "workspaceWrite" });
    await waitFor(() =>
      expect(useSessionAccessModeStore.getState().modes["fixture"]).toBe(
        "workspaceWrite",
      ),
    );
  });

  it("states when the chosen mode takes effect", async () => {
    renderControls();
    openOptions();
    fireEvent.click(screen.getByRole("button", { name: "Access options" }));
    expect(
      screen.getByText(/Applies from your next message with this harness./),
    ).toBeInTheDocument();
  });

  it("omits Ask when native policies do not admit it", async () => {
    renderControls();
    fireEvent.click(screen.getByRole("button", { name: "Access options" }));
    expect(
      screen.queryByRole("menuitemradio", { name: "Ask each time" }),
    ).toBeNull();
  });

  it("admits Ask only from the native policy capabilities", async () => {
    useWorkbenchStore.setState((state) => ({
      harnesses: state.harnesses.map((harness) => ({
        ...harness,
        accessModes: [
          {
            id: "ask",
            label: "Ask each time",
            description:
              "Review each permission request; no filesystem sandbox.",
          },
        ],
      })),
    }));
    renderControls();
    fireEvent.click(screen.getByRole("button", { name: "Access options" }));
    const ask = await screen.findByRole("menuitemradio", {
      name: "Ask each time",
    });
    expect(ask).toBeEnabled();
    expect(ask).toHaveAccessibleDescription(
      "Review each permission request; no filesystem sandbox.",
    );
    expect(
      screen.queryByRole("menuitemradio", { name: "Read-only" }),
    ).toBeNull();
  });

  it("hides the picker without an available binding", async () => {
    renderControls({ availableBinding: false });
    openOptions();
    expect(screen.queryByRole("button", { name: "Access options" })).toBeNull();
  });

  it("is not actionable before the persisted mode has been restored", async () => {
    useSessionAccessModeStore.setState({ ready: false });
    renderControls();
    openOptions();
    expect(
      screen.getByRole("button", { name: "Access options" }),
    ).toBeDisabled();
  });

  it("surfaces a save failure instead of silently reverting", async () => {
    vi.spyOn(
      getWorkbenchPorts().settings,
      "setSessionAccessModes",
    ).mockRejectedValue(new Error("nope"));
    renderControls();
    openOptions();
    fireEvent.click(screen.getByRole("button", { name: "Access options" }));
    fireEvent.click(
      await screen.findByRole("menuitemradio", { name: "Workspace edits" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Switch access" }));

    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(
        "Permission mode could not be saved.",
      ),
    );
  });
});

it("keeps the send binding's access modes when switching between Access and Add", async () => {
  useWorkbenchStore.setState({
    harnesses: [
      ...useWorkbenchStore.getState().harnesses,
      {
        id: "other",
        name: "Other",
        description: "ACP",
        accountModelDiscovery: false,
        source: "builtIn",
        transport: "acp",
        installState: "available",
        bindingId: "other-binding",
        accessModes: [
          {
            id: "ask",
            label: "Ask each time",
            description: "Review requests.",
            fallback: true,
          },
        ],
      },
    ],
  });
  const onChoose = vi.fn();
  renderControls({
    selectedHarnessId: "other",
    accessBindingId: "fixture",
    imageAction: { disabled: false, onChoose },
  });
  const access = screen.getByRole("button", { name: "Access options" });
  const add = screen.getByRole("button", { name: "Composer options" });
  fireEvent.click(access);
  expect(
    await screen.findByRole("menuitemradio", { name: "Workspace edits" }),
  ).toBeInTheDocument();
  expect(
    screen.queryByRole("menuitemradio", { name: "Ask each time" }),
  ).toBeNull();
  fireEvent.click(add);
  expect(screen.queryByRole("menu", { name: "Access mode" })).toBeNull();
  expect(screen.getAllByRole("menu")).toHaveLength(1);
  fireEvent.click(screen.getByRole("menuitem", { name: "Add images" }));
  expect(onChoose).toHaveBeenCalledOnce();
  expect(add).toHaveFocus();
  fireEvent.click(add);
  fireEvent.click(access);
  expect(
    screen.queryByRole("menu", { name: "Composer options menu" }),
  ).toBeNull();
  const menu = await screen.findByRole("menu", { name: "Access mode" });
  expect(screen.getAllByRole("menu")).toHaveLength(1);
  expect(
    await screen.findByRole("menuitemradio", { name: "Workspace edits" }),
  ).toBeInTheDocument();
  fireEvent.keyDown(menu, { key: "Escape" });
  expect(screen.queryByRole("menu")).toBeNull();
  expect(access).toHaveFocus();
});

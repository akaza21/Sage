import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import { decodeSessionConfig } from "../../domain/acpSessionConfig";
import { ChatHarnessControls } from "./ChatHarnessControls";
import {
  confirmedComposerModel,
  inheritComposerModel,
  useInheritedComposerModel,
} from "./inheritedComposerModel";

const noop = () => Promise.resolve();
const initialWorkbenchState = useWorkbenchStore.getState();

function parentConfig(
  model = "Synthetic replay",
  effort = "medium",
  selectable = false,
) {
  return decodeSessionConfig({
    sageAgentSessionId: "parent-session",
    runtimeEpoch: "1",
    configRevision: "1",
    modes: null,
    configOptions: [
      {
        id: "pi.model",
        name: "Model",
        category: "model",
        type: "select",
        currentValue: "fixture-model",
        options: [
          { value: "fixture-model", name: model },
          ...(selectable
            ? [{ value: "alternative", name: "Alternative model" }]
            : []),
        ],
      },
      {
        id: "pi.thinking",
        name: "Thinking",
        category: "effort",
        type: "select",
        currentValue: effort,
        options: [{ value: effort, name: effort }],
      },
    ],
  });
}

function Probe({ chatId }: { chatId: string }) {
  const inherited = useInheritedComposerModel(chatId);
  return <output>{inherited ? JSON.stringify(inherited) : "none"}</output>;
}

function renderControls(props: Record<string, unknown> = {}) {
  return render(
    <ChatHarnessControls
      availableBinding={false}
      canPrompt
      discoveryPhase="ready"
      modelLabelScope="project/side-1"
      inheritScope="side-1"
      sessionHarnessId={null}
      sessionModelId={null}
      selectedHarnessId="fixture"
      selectedModelId="fixture-model"
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
    />,
  );
}

beforeEach(() => {
  useWorkbenchStore.setState({
    harnesses: [
      {
        id: "fixture",
        name: "Fixture",
        description: "",
        installState: "available",
        source: "builtIn",
        transport: "acp",
        bindingId: "fixture",
        accountModelDiscovery: false,
      },
    ],
    selectedHarnessId: "fixture",
    models: [],
  });
});

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialWorkbenchState, true);
});

describe("inherited composer model (Codex s2/f0244, s2/f0066)", () => {
  it("keeps only the parent's confirmed model and effort labels", () => {
    expect(confirmedComposerModel(parentConfig())).toEqual({
      model: "Synthetic replay",
      effort: "medium",
      id: "fixture-model",
    });
    expect(confirmedComposerModel(null)).toBeNull();
    const unconfirmed = decodeSessionConfig({
      sageAgentSessionId: "parent-session",
      runtimeEpoch: "1",
      configRevision: "1",
      modes: null,
      configOptions: [],
    });
    expect(confirmedComposerModel(unconfirmed)).toBeNull();
  });

  it("records nothing for an unconfirmed parent and bounds the record", () => {
    inheritComposerModel("unconfirmed", null);
    render(<Probe chatId="unconfirmed" />);
    expect(screen.getByRole("status")).toHaveTextContent("none");
    cleanup();
    for (let index = 0; index < 20; index += 1)
      inheritComposerModel(`chat-${index}`, parentConfig(`Model ${index}`));
    render(<Probe chatId="chat-3" />);
    expect(screen.getByRole("status")).toHaveTextContent("none");
    cleanup();
    render(<Probe chatId="chat-19" />);
    expect(screen.getByRole("status")).toHaveTextContent("Model 19");
  });

  it("labels a new fork's composer with the parent's model and effort", () => {
    inheritComposerModel("side-1", parentConfig());
    renderControls();
    const model = screen.getByLabelText("Model", { exact: true });
    expect(model).toHaveTextContent("Synthetic replay");
    expect(model).toHaveTextContent("Medium");
    expect(model).not.toHaveTextContent(/^Model$/);
    expect(model).toHaveAttribute("data-readonly-label", "true");
  });

  it.each(["pending", "no choices", "transition", "locked"])(
    "dims a known model for %s unless it is an inherited or locked label",
    (state) => {
      useWorkbenchStore.setState({
        projectTransitioning: state === "transition",
      });
      renderControls({
        inheritScope: "no-inheritance",
        sessionHarnessId: "fixture",
        sessionModelId: "fixture-model",
        sessionConfig: parentConfig("Synthetic replay", "medium", true),
        modePending: state === "pending",
        turnBusy: state === "locked",
        ...(state === "no choices"
          ? {
              sessionConfig: {
                ...parentConfig(),
                configOptions: [],
                confirmedModel: {
                  id: "fixture-model",
                  name: "Synthetic replay",
                },
              },
            }
          : {}),
      });
      const model = screen.getByLabelText("Model", { exact: true });
      expect(model).toBeDisabled();
      expect(model.getAttribute("title")).toMatch(/^Synthetic replay/);
      if (state === "locked")
        expect(model).toHaveAttribute("data-readonly-label", "true");
      else expect(model).not.toHaveAttribute("data-readonly-label", "true");
    },
  );

  it.each(["pending", "transition"])(
    "dims even an inherited label during %s",
    (state) => {
      inheritComposerModel("side-1", parentConfig());
      useWorkbenchStore.setState({
        projectTransitioning: state === "transition",
      });
      renderControls({ modePending: state === "pending" });
      const model = screen.getByLabelText("Model", { exact: true });
      expect(model).toBeDisabled();
      expect(model).toHaveTextContent("Synthetic replay");
      expect(model).not.toHaveAttribute("data-readonly-label", "true");
    },
  );

  it("keeps a fork's confirmed parent model and effort ahead of catalog labels", () => {
    inheritComposerModel("fork-catalog", parentConfig());
    useWorkbenchStore.setState({
      models: [{ id: "fixture-model", name: "Catalog alias" }],
    });
    renderControls({ inheritScope: "fork-catalog" });
    const model = screen.getByLabelText("Model", { exact: true });
    expect(model).toHaveTextContent("Synthetic replay");
    expect(model).toHaveTextContent("Medium");
    expect(model).not.toHaveTextContent("Catalog alias");
    expect(screen.queryByRole("button", { name: "Model" })).toBeNull();
    expect(model).not.toHaveAttribute("tabindex");
  });

  it("labels a side chat with the parent's model only, never its effort", () => {
    inheritComposerModel("side-1", parentConfig(), {
      startsOnModelId: "fixture-model",
    });
    renderControls();
    const model = screen.getByLabelText("Model", { exact: true });
    expect(model).toHaveTextContent("Synthetic replay");
    expect(model).not.toHaveTextContent("Medium");
    expect(model).toHaveAttribute("data-readonly-label", "true");
  });

  it("keeps the inherited model through an empty initial configuration (R3-02)", () => {
    inheritComposerModel("side-empty", parentConfig(), {
      startsOnModelId: "fixture-model",
    });
    renderControls({
      inheritScope: "side-empty",
      sessionHarnessId: "fixture",
      sessionModelId: "fixture-model",
      sessionConfig: {
        ...parentConfig(),
        configOptions: [],
        confirmedEffort: "high",
      },
    });
    const model = screen.getByLabelText("Model", { exact: true });
    expect(model).toHaveTextContent("Synthetic replay");
    expect(model).toHaveTextContent("High");
    expect(model).not.toHaveTextContent("Medium");
    expect(model).toHaveAttribute("data-readonly-label", "true");
  });

  it.each([null, "other-model"])(
    "inherits nothing for a side chat that starts on model %s",
    (startsOnModelId) => {
      inheritComposerModel("side-model-mismatch", parentConfig(), {
        startsOnModelId,
      });
      render(<Probe chatId="side-model-mismatch" />);
      expect(screen.getByRole("status")).toHaveTextContent("none");
    },
  );

  it("yields to the chat's own confirmed configuration", () => {
    inheritComposerModel("side-1", parentConfig());
    renderControls({
      sessionHarnessId: "fixture",
      sessionModelId: "fixture-model",
      sessionConfig: parentConfig("Own model", "high"),
    });
    const model = screen.getByLabelText("Model", { exact: true });
    expect(model).toHaveTextContent("Own model");
    expect(model).not.toHaveTextContent("Synthetic replay");
  });

  it("drops the inherited label after selecting a different model id (R3-02)", () => {
    inheritComposerModel("side-1", parentConfig(), {
      startsOnModelId: "fixture-model",
    });
    renderControls({ selectedModelId: "other-model" });
    expect(screen.getByLabelText("Model", { exact: true })).toHaveTextContent(
      /^Model$/,
    );
  });

  it("shows the plain placeholder when nothing was inherited", () => {
    renderControls({ inheritScope: "side-unknown" });
    expect(screen.getByLabelText("Model", { exact: true })).toHaveTextContent(
      /^Model$/,
    );
  });
});

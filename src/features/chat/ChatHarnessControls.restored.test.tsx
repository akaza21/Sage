import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { useSessionAccessModeStore } from "../../app/sessionAccessModeStore";
import { useWorkbenchStore } from "../../app/store";
import type { AcpChatPage } from "../../domain/acp";
import {
  decodeRestoredSessionConfig,
  decodeSavedChoiceNotice,
} from "../../domain/acpRestoredSessionConfig";
import { DEFAULT_SESSION_ACCESS_MODE } from "../../domain/sessionAccessMode";
import { ChatHarnessControls } from "./ChatHarnessControls";
import {
  PENDING_NOTE,
  RESTORED_NOTE,
  restoredSessionConfigView,
  withSessionConfigNotice,
} from "./restoredSessionConfigView";

const initialWorkbenchState = useWorkbenchStore.getState();
const noop = () => Promise.resolve();

const restored = decodeRestoredSessionConfig({
  sageAgentSessionId: "session-1",
  awaiting: "resume",
  modes: {
    currentModeId: "plan",
    availableModes: [
      { id: "default", name: "Default" },
      { id: "plan", name: "Plan" },
    ],
  },
  configOptions: [
    {
      id: "model",
      name: "Model",
      category: "model",
      type: "select",
      currentValue: "fixture-model-deep",
      options: [
        { value: "fixture-model", name: "Synthetic replay" },
        { value: "fixture-model-deep", name: "Synthetic replay deep" },
      ],
    },
    {
      id: "effort",
      name: "Effort",
      category: "thought_level",
      type: "select",
      currentValue: "high",
      options: [
        { value: "low", name: "Low" },
        { value: "medium", name: "Medium" },
        { value: "high", name: "High" },
      ],
    },
    {
      id: "fast",
      name: "Fast",
      type: "boolean",
      currentValue: true,
    },
  ],
});

function coldPage(overrides: Partial<AcpChatPage> = {}): AcpChatPage {
  return {
    projectId: "project",
    chatId: "chat",
    title: "Chat",
    projectionRevision: "4",
    currentSessionId: "session-1",
    sessions: [
      {
        id: "session-1",
        bindingId: "fixture",
        modelId: null,
        state: "ready",
        residency: "cold",
        runtimeEpoch: null,
        legacy: false,
      },
    ],
    turns: [],
    nextCursor: null,
    sessionConfig: null,
    restoredSessionConfig: restored,
    ...overrides,
  } as unknown as AcpChatPage;
}

beforeEach(() => {
  useWorkbenchStore.setState({
    harnesses: [
      {
        id: "restored-test",
        name: "Restored fixture",
        description: "",
        installState: "available",
        source: "builtIn",
        transport: "acp",
        bindingId: "fixture",
        accountModelDiscovery: false,
        accessModes: [
          {
            id: "readOnly" as const,
            label: "Read-only",
            description: "Read workspace files; no writes or network.",
            fallback: true,
          },
        ],
      },
    ],
  });
  useSessionAccessModeStore.setState({
    modes: { fixture: DEFAULT_SESSION_ACCESS_MODE },
    ready: true,
    error: null,
  });
});

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialWorkbenchState, true);
});

it("decodes a restored config without a runtime epoch or revision", () => {
  expect(restored?.sageAgentSessionId).toBe("session-1");
  expect(restored?.awaiting).toBe("resume");
  expect(
    decodeRestoredSessionConfig({
      ...restored,
      runtimeEpoch: "1",
      configRevision: "1",
    }),
  ).toBeNull();
  expect(
    decodeRestoredSessionConfig({ ...restored, awaiting: undefined }),
  ).toBeNull();
});

it("says a live session that has not reported will apply the stored choice", () => {
  const view = restoredSessionConfigView(
    coldPage({
      restoredSessionConfig: { ...restored!, awaiting: "report" },
    }),
    "fixture",
    null,
  );
  expect(view?.restored).toBe(PENDING_NOTE);
  expect(restoredSessionConfigView(coldPage(), "fixture", null)?.restored).toBe(
    RESTORED_NOTE,
  );
});

it("views the restored config only for the session the selection continues", () => {
  const view = restoredSessionConfigView(coldPage(), "fixture", null);
  expect(view?.restored).toBe(RESTORED_NOTE);
  expect(view?.configOptions.map((option) => option.id)).toEqual([
    "model",
    "effort",
    "fast",
  ]);
  expect(restoredSessionConfigView(coldPage(), "other", null)).toBeNull();
  expect(restoredSessionConfigView(coldPage(), "fixture", "x")).toBeNull();
  expect(
    restoredSessionConfigView(
      coldPage({ currentSessionId: null }),
      "fixture",
      null,
    ),
  ).toBeNull();
});

it("shows a cold chat's stored model and effort as restored, and a pick goes to the stored choice", async () => {
  const onSetSessionConfigOption = vi.fn().mockResolvedValue(undefined);
  render(
    <ChatHarnessControls
      availableBinding
      canPrompt
      discoveryPhase="ready"
      modelLabelScope="project/chat/session-1"
      sessionHarnessId={null}
      sessionModelId={null}
      selectedHarnessId="restored-test"
      selectedModelId={null}
      projectName={null}
      turnBusy={false}
      sessionConfig={restoredSessionConfigView(coldPage(), "fixture", null)}
      modePending={false}
      modeError={null}
      focused={false}
      onSetSessionMode={noop}
      onSetSessionConfigOption={onSetSessionConfigOption}
      onSelectHarness={noop}
      onSelectModel={() => {}}
      onModeRemoved={() => {}}
    />,
  );
  const model = screen.getByLabelText("Model", { exact: true });
  expect(model).toHaveTextContent("Synthetic replay deep");
  expect(model).toHaveAttribute("data-restored", "true");
  expect(model).toBeEnabled();
  expect(model.getAttribute("aria-description")).toContain(RESTORED_NOTE);
  const effort = screen.getByRole("button", {
    name: "Thinking level: High",
  });
  expect(effort).toHaveAttribute("aria-description", RESTORED_NOTE);
  expect(
    screen.queryByRole("button", { name: "Thinking level" }),
  ).not.toBeInTheDocument();
  fireEvent.click(model);
  fireEvent.click(
    await screen.findByRole("option", { name: /^Synthetic replay$/ }),
  );
  expect(onSetSessionConfigOption).toHaveBeenCalledWith("model", {
    value: "fixture-model",
  });
});

function renderControls(
  sessionConfig: ReturnType<typeof restoredSessionConfigView>,
) {
  return render(
    <ChatHarnessControls
      availableBinding
      canPrompt
      discoveryPhase="ready"
      modelLabelScope="project/chat/session-1"
      sessionHarnessId={null}
      sessionModelId={null}
      selectedHarnessId="restored-test"
      selectedModelId={null}
      projectName={null}
      turnBusy={false}
      sessionConfig={sessionConfig}
      modePending={false}
      modeError={null}
      focused={false}
      onSetSessionMode={noop}
      onSetSessionConfigOption={noop}
      onSelectHarness={noop}
      onSelectModel={() => {}}
      onModeRemoved={() => {}}
    />,
  );
}

it("marks the restored mode and other options with the same note", async () => {
  renderControls(restoredSessionConfigView(coldPage(), "fixture", null));
  fireEvent.click(screen.getByRole("button", { name: "Composer options" }));
  const mode = await screen.findByRole("group", { name: "Session mode" });
  expect(mode).toHaveAttribute("aria-description", RESTORED_NOTE);
  expect(mode).toHaveAttribute("data-restored", "true");
  const fast = screen.getByRole("group", { name: "Session fast" });
  expect(fast).toHaveAttribute("aria-description", RESTORED_NOTE);
  expect(fast).toHaveAttribute("data-restored", "true");
});

it("decodes a notice only with its four bounded fields", () => {
  const notice = {
    configId: "model",
    saved: "retired",
    current: "fixture-model",
    reason: "notOffered",
  };
  expect(decodeSavedChoiceNotice(notice)).toEqual(notice);
  expect(decodeSavedChoiceNotice({ ...notice, reason: "other" })).toBeNull();
  expect(
    decodeSavedChoiceNotice({ ...notice, saved: "x".repeat(1025) }),
  ).toBeNull();
  expect(decodeSavedChoiceNotice({ ...notice, extra: 1 })).toBeNull();
});

it("words a refused saved model apart from one no longer offered", () => {
  const view = restoredSessionConfigView(coldPage(), "fixture", null)!;
  expect(
    withSessionConfigNotice(view, {
      configId: "model",
      saved: "fixture-model-deep",
      current: "fixture-model",
      reason: "rejected",
    })?.notice,
  ).toBe(
    "The harness refused the saved model Synthetic replay deep; this chat uses Synthetic replay.",
  );
});

it("shows the notice about a saved model the session no longer offers", () => {
  const notice =
    "The saved model retired isn't offered any more; this chat uses Synthetic replay.";
  renderControls(
    withSessionConfigNotice(
      restoredSessionConfigView(coldPage(), "fixture", null),
      {
        configId: "model",
        saved: "retired",
        current: "fixture-model",
        reason: "notOffered",
      },
    ),
  );
  expect(screen.getByRole("status")).toHaveTextContent(notice);
  expect(
    screen
      .getByLabelText("Model", { exact: true })
      .getAttribute("aria-description"),
  ).toContain(notice);
});

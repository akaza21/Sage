import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { useSessionAccessModeStore } from "../../app/sessionAccessModeStore";
import { useWorkbenchStore } from "../../app/store";
import type { AcpChatPage } from "../../domain/acp";
import { decodeRestoredSessionConfig } from "../../domain/acpSessionConfig";
import { DEFAULT_SESSION_ACCESS_MODE } from "../../domain/sessionAccessMode";
import { ChatHarnessControls } from "./ChatHarnessControls";
import {
  RESTORED_NOTE,
  restoredSessionConfigView,
} from "./restoredSessionConfigView";

const initialWorkbenchState = useWorkbenchStore.getState();
const noop = () => Promise.resolve();

// What a parked chat's page carries after an app restart: the config the
// session last reported, with its stored model and effort as current values.
const restored = decodeRestoredSessionConfig({
  sageAgentSessionId: "session-1",
  modes: null,
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
  expect(
    decodeRestoredSessionConfig({
      ...restored,
      runtimeEpoch: "1",
      configRevision: "1",
    }),
  ).toBeNull();
});

it("views the restored config only for the session the selection continues", () => {
  const view = restoredSessionConfigView(coldPage(), "fixture", null);
  expect(view?.restored).toBe(true);
  expect(view?.configOptions.map((option) => option.id)).toEqual([
    "model",
    "effort",
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
      // Cold: no runtime, so the session owns no harness selection yet.
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
  // No "Thinking level" placeholder waiting for a session.
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

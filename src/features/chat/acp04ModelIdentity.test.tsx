import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it } from "vitest";

import {
  ACP04_SNAPSHOT_EXPRESSION,
  acceptModeASnapshot,
  type Acp04Snapshot,
  createModeAState,
} from "../../../scripts/source-functional-acp04-live-probe.mjs";
import { useWorkbenchStore } from "../../app/store";
import { decodeSessionConfig } from "../../domain/acpSessionConfig";
import { ChatHarnessControls } from "./ChatHarnessControls";

const CODEX_VERSION = "codex 0.167.0";
const MODEL_ID = "gpt-5.6";
const ECHO = "SAGE_ACP04_A_TURN_1";
const initialState = useWorkbenchStore.getState();
const noop = () => Promise.resolve();

function confirmedSession() {
  return decodeSessionConfig({
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
        currentValue: MODEL_ID,
        options: [{ value: MODEL_ID, name: "gpt-5.6" }],
      },
      {
        id: "effort",
        name: "Effort",
        category: "effort",
        type: "select",
        currentValue: "high",
        options: [{ value: "high", name: "High" }],
      },
    ],
  });
}

function ChatPane({ confirmed }: { confirmed: boolean }) {
  return (
    <section className="agent-chat-pane agent-chat-pane--focused">
      <span aria-label="Harness status: Ready">Ready</span>
      <section className="chat-surface" aria-label="Agent chat">
        <section
          className="acp-chat-projection"
          aria-label="Agent chat Fresh"
          data-project-id="project-1"
          data-chat-id="chat-1"
          data-current-session-id={confirmed ? "session-1" : ""}
          data-projection-revision={confirmed ? "2" : "1"}
        >
          {confirmed ? (
            <section
              data-turn-id="turn-1"
              data-session-id="session-1"
              data-runtime-epoch="1"
              data-turn-ordinal="1"
              data-turn-state="completed"
            >
              <article data-item-id="item-1" data-message-role="user">
                <div className="acp-markdown">Reply with exactly: {ECHO}</div>
              </article>
              <article data-item-id="item-2" data-message-role="assistant">
                <div className="acp-markdown">{ECHO}</div>
              </article>
            </section>
          ) : null}
        </section>
        <ChatHarnessControls
          availableBinding
          canPrompt
          discoveryPhase="ready"
          modelLabelScope="project-1/chat-1"
          sessionHarnessId={confirmed ? "codex" : null}
          sessionModelId={confirmed ? MODEL_ID : null}
          selectedHarnessId="codex"
          selectedModelId={MODEL_ID}
          projectName={null}
          turnBusy={false}
          sessionConfig={confirmed ? confirmedSession() : null}
          modePending={false}
          modeError={null}
          focused
          onSetSessionMode={noop}
          onSetSessionConfigOption={noop}
          onSelectHarness={noop}
          onSelectModel={() => {}}
          onModeRemoved={() => {}}
        />
        <textarea aria-label="Message agent" />
      </section>
    </section>
  );
}

beforeEach(() => {
  // jsdom has no layout, so the probe's innerText reads as its text content.
  Object.defineProperty(HTMLElement.prototype, "innerText", {
    configurable: true,
    get(this: HTMLElement) {
      return this.textContent;
    },
  });
  useWorkbenchStore.setState({
    selectedHarnessId: "codex",
    models: [{ id: MODEL_ID, name: "gpt-5.6" }],
    harnesses: [
      {
        id: "codex",
        name: "Codex",
        description: "",
        installState: "available",
        source: "builtIn",
        transport: "acp",
        bindingId: "codex",
        accountModelDiscovery: false,
        accessModes: [],
      },
    ],
  });
});

afterEach(() => {
  cleanup();
  Reflect.deleteProperty(HTMLElement.prototype, "innerText");
  useWorkbenchStore.setState(initialState, true);
});

it("keeps one model identity while the composer confirms the model's name", () => {
  const view = render(<ChatPane confirmed={false} />);
  const before = window.eval(ACP04_SNAPSHOT_EXPRESSION) as Acp04Snapshot;
  expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
    "Model",
  );
  const state = createModeAState(
    [ECHO, "SAGE_ACP04_A_TURN_2"],
    before,
    CODEX_VERSION,
  );

  view.rerender(<ChatPane confirmed />);
  const after = window.eval(ACP04_SNAPSHOT_EXPRESSION) as Acp04Snapshot;
  expect(after.model.id).toBe(before.model.id);
  expect(after.model.text).not.toBe(before.model.text);

  const next = acceptModeASnapshot(state, after);
  expect(next.phase).toBe("await-second");
  expect(next.model).toEqual(after.model);
});

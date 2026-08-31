import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { JSDOM } from "jsdom";

import {
  ACP04_MODE_A,
  ACP04_MODE_B_CANCEL_PROMPT,
  ACP04_MODE_B_RECOVERY_ECHO,
  ACP04_SNAPSHOT_EXPRESSION,
  acceptModeBSnapshot,
  acceptModeASnapshot,
  createAcp04Ui,
  createAcp04Receipt,
  createModeBState,
  createModeAState,
  freshChatReadyExpression,
  fillPromptExpression,
  finalizeAcp04Collection,
  parseAcp04ProbeArguments,
  probeAcp04Snapshot,
  runAcp04ModeB,
  runAcp04ModeA,
} from "./source-functional-acp04-live-probe.mjs";
import {
  attestNativeDiagnostics,
  createFinalAcp04Receipt,
  decodeTerminalZeroCleanup,
  preflightCodex,
  revalidateCodexPreflight,
  validatePendingAcp04Candidate,
  validateRuntimeObservation,
} from "./source-functional-acp04-finalizer.mjs";

const SHA = "a".repeat(64);
const GIT = "b".repeat(40);
const ECHOES = ["SAGE_ACP04_A_TURN_1", "SAGE_ACP04_A_TURN_2"];
const CODEX_VERSION = "codex 0.146.0";

function argumentsFor(overrides = {}) {
  const values = {
    port: "9229",
    output: "C:\\Sage\\local-evidence\\ACP04-live-test",
    "source-commit": GIT,
    "source-tree": GIT,
    executable: "C:\\Sage\\target\\debug\\sage.exe",
    "source-root": "C:\\Sage",
    "executable-bytes": "100",
    "executable-sha256": SHA,
    "node-bytes": "100",
    "node-sha256": SHA,
    "codex-path": "C:\\Tools\\codex.cmd",
    "codex-bytes": "100",
    "codex-sha256": SHA,
    "profile-root": "C:\\Sage\\local-evidence\\ACP04-live-test\\profile",
    "profile-identity-artifact":
      "C:\\Sage\\local-evidence\\ACP04-live-test\\acp04-profile-identity.json",
    "sage-stdout-log":
      "C:\\Sage\\local-evidence\\ACP04-live-test\\sage.stdout.log",
    "sage-stderr-log":
      "C:\\Sage\\local-evidence\\ACP04-live-test\\sage.stderr.log",
    "sage-process-id": "4100",
    "sage-process-created": "2026-08-30T12:05:00.000Z",
    "expected-origin": "http://127.0.0.1:5173/",
    "runtime-mode": "tauri-dev-split",
    "frontend-ready": "true",
    "receipt-name": "machine-receipt.pending.json",
    "deadline-ms": "90000",
    mode: ACP04_MODE_A,
    ...overrides,
  };
  return Object.entries(values).flatMap(([key, value]) => [`--${key}`, value]);
}

function snapshot({
  projectId = "project-1",
  chatId = "chat-1",
  currentSessionId = "session-1",
  projectionRevision = "1",
  turns = [],
  failures = [],
} = {}) {
  return {
    projectId,
    chatId,
    currentSessionId,
    projectionRevision,
    harness: { id: "codex", text: `Codex \u2014 ${CODEX_VERSION}` },
    model: { id: "gpt-5.6", text: "gpt-5.6" },
    harnessStatus: "Live",
    composerEnabled: true,
    failures,
    turns,
  };
}

function completedTurn(
  ordinal,
  prompt,
  answer,
  sessionId = "session-1",
  runtimeEpoch = "7",
) {
  return {
    id: `turn-${ordinal}`,
    sessionId,
    runtimeEpoch,
    ordinal,
    state: "Completed",
    items: [
      { id: `item-${ordinal}-user`, role: "user", text: prompt },
      { id: `item-${ordinal}-assistant`, role: "assistant", text: answer },
    ],
  };
}

function activeTurn(ordinal, prompt, sessionId = "session-1") {
  return {
    id: `turn-${ordinal}`,
    sessionId,
    runtimeEpoch: "7",
    ordinal,
    state: "Running",
    items: [{ id: `item-${ordinal}-user`, role: "user", text: prompt }],
  };
}

function cancelledTurn(ordinal, prompt, sessionId = "session-1") {
  return {
    ...activeTurn(ordinal, prompt, sessionId),
    state: "Cancelled",
  };
}

function completeState() {
  let state = createModeAState(
    ECHOES,
    snapshot({ currentSessionId: "" }),
    CODEX_VERSION,
  );
  for (let index = 0; index < 2; index += 1) {
    state = acceptModeASnapshot(
      state,
      snapshot({
        projectionRevision: String(index + 2),
        turns: ECHOES.slice(0, index + 1).map((echo, turnIndex) =>
          completedTurn(turnIndex + 1, `Reply with exactly: ${echo}`, echo),
        ),
      }),
    );
  }
  return state;
}

function zeroBasedCompleteState() {
  let state = createModeAState(
    ECHOES,
    snapshot({ currentSessionId: "" }),
    CODEX_VERSION,
  );
  for (let index = 0; index < ECHOES.length; index += 1) {
    state = acceptModeASnapshot(
      state,
      snapshot({
        projectionRevision: String(index + 2),
        turns: ECHOES.slice(0, index + 1).map((echo, turnIndex) =>
          completedTurn(turnIndex, `Reply with exactly: ${echo}`, echo),
        ),
      }),
    );
  }
  return state;
}

test("argument contract binds a Codex launcher for collector-owned preflight", () => {
  const parsed = parseAcp04ProbeArguments(argumentsFor());
  assert.equal(parsed.mode, "two-turn");
  assert.equal(parsed.codex.path, "C:\\Tools\\codex.cmd");
  assert.equal("version" in parsed.codex, false);
  assert.equal(parsed.sageProcess.processId, 4100);
  assert.throws(
    () => parseAcp04ProbeArguments(argumentsFor({ mode: "restart" })),
    /acp04-arguments-invalid/,
  );
});

test("Mode A accepts exactly two turns with one project, chat, session, and model", () => {
  const state = completeState();
  assert.equal(state.phase, "complete");
  assert.deepEqual(state.turnIds, ["turn-1", "turn-2"]);
  assert.deepEqual(state.itemIds, [
    "item-1-user",
    "item-1-assistant",
    "item-2-user",
    "item-2-assistant",
  ]);
  assert.equal(state.projectId, "project-1");
  assert.equal(state.chatId, "chat-1");
  assert.equal(state.sessionId, "session-1");
  assert.equal(state.runtimeEpoch, "7");
  assert.equal(state.projectionRevision, "3");
});

test("Mode A accepts the durable zero-based Turn ordinal contract", () => {
  const state = zeroBasedCompleteState();
  assert.equal(state.phase, "complete");
  assert.deepEqual(state.turnOrdinals, [0, 1]);
  assert.equal(createModeBState(state).phase, "await-active-third");
});

test("Mode B accepts one active cancellation then an exact recovery echo at a higher runtime epoch", () => {
  let state = createModeBState(completeState());
  const firstTwo = ECHOES.map((echo, index) =>
    completedTurn(index + 1, `Reply with exactly: ${echo}`, echo),
  );
  state = acceptModeBSnapshot(
    state,
    snapshot({
      projectionRevision: "4",
      turns: [...firstTwo, activeTurn(3, ACP04_MODE_B_CANCEL_PROMPT)],
    }),
  );
  assert.equal(state.phase, "await-terminal-third");
  const cancelling = acceptModeBSnapshot(
    state,
    snapshot({
      projectionRevision: "5",
      turns: [
        ...firstTwo,
        {
          ...activeTurn(3, ACP04_MODE_B_CANCEL_PROMPT),
          state: "Cancelling",
        },
      ],
    }),
  );
  assert.equal(cancelling.phase, "await-terminal-third");
  state = acceptModeBSnapshot(
    cancelling,
    snapshot({
      projectionRevision: "6",
      turns: [...firstTwo, cancelledTurn(3, ACP04_MODE_B_CANCEL_PROMPT)],
    }),
  );
  assert.equal(state.phase, "await-fourth");
  state = acceptModeBSnapshot(
    state,
    snapshot({
      projectionRevision: "7",
      turns: [
        ...firstTwo,
        cancelledTurn(3, ACP04_MODE_B_CANCEL_PROMPT),
        completedTurn(
          4,
          `Reply with exactly: ${ACP04_MODE_B_RECOVERY_ECHO}`,
          ACP04_MODE_B_RECOVERY_ECHO,
          "session-1",
          "8",
        ),
      ],
    }),
  );
  assert.equal(state.phase, "complete");
  assert.equal(state.cancelledTurnId, "turn-3");
  assert.equal(state.cancelledState, "Cancelled");
  assert.equal(state.runtimeEpoch, "8");
  assert.deepEqual(state.turnIds, ["turn-1", "turn-2", "turn-3", "turn-4"]);
});

test("Mode B rejects turn identity reuse and recovery without an epoch restart", () => {
  const initial = createModeBState(completeState());
  const firstTwo = ECHOES.map((echo, index) =>
    completedTurn(index + 1, `Reply with exactly: ${echo}`, echo),
  );
  assert.throws(
    () =>
      acceptModeBSnapshot(
        initial,
        snapshot({
          projectionRevision: "4",
          turns: [
            ...firstTwo,
            { ...activeTurn(3, ACP04_MODE_B_CANCEL_PROMPT), id: "turn-2" },
          ],
        }),
      ),
    /acp04-snapshot-invalid:identity-order/,
  );
  const cancelled = acceptModeBSnapshot(
    acceptModeBSnapshot(
      initial,
      snapshot({
        projectionRevision: "4",
        turns: [...firstTwo, activeTurn(3, ACP04_MODE_B_CANCEL_PROMPT)],
      }),
    ),
    snapshot({
      projectionRevision: "5",
      turns: [...firstTwo, cancelledTurn(3, ACP04_MODE_B_CANCEL_PROMPT)],
    }),
  );
  assert.throws(
    () =>
      acceptModeBSnapshot(
        cancelled,
        snapshot({
          projectionRevision: "6",
          turns: [
            ...firstTwo,
            cancelledTurn(3, ACP04_MODE_B_CANCEL_PROMPT),
            completedTurn(
              4,
              `Reply with exactly: ${ACP04_MODE_B_RECOVERY_ECHO}`,
              ACP04_MODE_B_RECOVERY_ECHO,
            ),
          ],
        }),
      ),
    /acp04-mode-b-runtime-restart-missing/,
  );
});

test("Mode B uses the exact Cancel control before the fourth normal composer turn", async () => {
  const firstTwo = ECHOES.map((echo, index) =>
    completedTurn(index + 1, `Reply with exactly: ${echo}`, echo),
  );
  let stage = 0;
  const calls = [];
  const driver = {
    async fillPrompt(prompt) {
      calls.push(`fill:${prompt}`);
    },
    async waitForSendReady() {
      calls.push("send-ready");
    },
    async send() {
      stage += 1;
      calls.push(`send:${stage}`);
    },
    async waitForCancelReady() {
      calls.push("cancel-ready");
    },
    async cancel() {
      stage += 1;
      calls.push("cancel");
    },
    async snapshot() {
      calls.push(`snapshot:${stage}`);
      if (stage === 1)
        return snapshot({
          projectionRevision: "4",
          turns: [...firstTwo, activeTurn(3, ACP04_MODE_B_CANCEL_PROMPT)],
        });
      if (stage === 2)
        return snapshot({
          projectionRevision: "5",
          turns: [...firstTwo, cancelledTurn(3, ACP04_MODE_B_CANCEL_PROMPT)],
        });
      return snapshot({
        projectionRevision: "6",
        turns: [
          ...firstTwo,
          cancelledTurn(3, ACP04_MODE_B_CANCEL_PROMPT),
          completedTurn(
            4,
            `Reply with exactly: ${ACP04_MODE_B_RECOVERY_ECHO}`,
            ACP04_MODE_B_RECOVERY_ECHO,
            "session-1",
            "8",
          ),
        ],
      });
    },
    async runtimeObservation() {
      calls.push("runtime-observation");
      return runtimeObservation("8");
    },
  };
  const result = await runAcp04ModeB({}, Date.now() + 5_000, completeState(), {
    driver,
    codexVersion: CODEX_VERSION,
  });
  assert.equal(result.state.phase, "complete");
  assert.deepEqual(calls, [
    `fill:${ACP04_MODE_B_CANCEL_PROMPT}`,
    "send-ready",
    "send:1",
    "snapshot:1",
    "cancel-ready",
    "cancel",
    "snapshot:2",
    `fill:Reply with exactly: ${ACP04_MODE_B_RECOVERY_ECHO}`,
    "send-ready",
    "send:3",
    "snapshot:3",
    "runtime-observation",
  ]);
});

test("Mode A rejects session churn and named protocol/recovery failures", () => {
  const first = acceptModeASnapshot(
    createModeAState(ECHOES, snapshot({ currentSessionId: "" }), CODEX_VERSION),
    snapshot({
      projectionRevision: "2",
      turns: [completedTurn(1, `Reply with exactly: ${ECHOES[0]}`, ECHOES[0])],
    }),
  );
  assert.throws(
    () =>
      acceptModeASnapshot(
        first,
        snapshot({
          currentSessionId: "session-2",
          projectionRevision: "3",
          turns: [
            completedTurn(1, `Reply with exactly: ${ECHOES[0]}`, ECHOES[0]),
            completedTurn(
              2,
              `Reply with exactly: ${ECHOES[1]}`,
              ECHOES[1],
              "session-2",
            ),
          ],
        }),
      ),
    /acp04-session-churn/,
  );
  assert.throws(
    () =>
      createModeAState(
        ECHOES,
        snapshot({ currentSessionId: "", failures: ["ProtocolFailure"] }),
        CODEX_VERSION,
      ),
    /acp04-observed-failure/,
  );
});

test("Mode A rejects stale revisions, extra turns, and empty or duplicate IDs", () => {
  const initial = snapshot({ currentSessionId: "", projectionRevision: "10" });
  const first = completedTurn(1, `Reply with exactly: ${ECHOES[0]}`, ECHOES[0]);
  assert.throws(
    () =>
      acceptModeASnapshot(
        createModeAState(ECHOES, initial, CODEX_VERSION),
        snapshot({ projectionRevision: "10", turns: [first] }),
      ),
    /acp04-projection-stale/,
  );
  assert.throws(
    () =>
      acceptModeASnapshot(
        createModeAState(ECHOES, initial, CODEX_VERSION),
        snapshot({
          projectionRevision: "11",
          turns: [
            first,
            {
              ...first,
              id: "turn-extra",
              ordinal: 2,
              items: first.items.map((item) => ({
                ...item,
                id: `${item.id}-extra`,
              })),
            },
          ],
        }),
      ),
    /acp04-turn-cardinality/,
  );
  const bad = {
    ...first,
    items: first.items.map((item) => ({ ...item, id: "" })),
  };
  assert.throws(
    () =>
      acceptModeASnapshot(
        createModeAState(ECHOES, initial, CODEX_VERSION),
        snapshot({ projectionRevision: "11", turns: [bad] }),
      ),
    /acp04-snapshot-invalid/,
  );
});

test("snapshot probe propagates malformed observations instead of retrying them", async () => {
  const observed = snapshot();
  assert.deepEqual(
    await probeAcp04Snapshot(
      { evaluate: async () => observed },
      Date.now() + 1_000,
    ),
    observed,
  );
  await assert.rejects(
    () =>
      probeAcp04Snapshot(
        { evaluate: async () => ({ ...observed, turns: "bad" }) },
        Date.now() + 1_000,
      ),
    /acp04-snapshot-invalid:turns/,
  );
});

test("Mode A uses behavior seams for Editor to one seeded Agent chat", async () => {
  let sent = 0;
  const calls = [];
  const driver = {
    async waitForEditorReady() {
      calls.push("editor-ready");
    },
    async enterAgentFromEditor() {
      calls.push("enter-agent");
    },
    async waitForSeededAgentChat() {
      calls.push("seeded-chat");
    },
    async seededChatId() {
      calls.push("seeded-chat-id");
      return "chat-seeded";
    },
    async createFreshChat() {
      calls.push("new-chat");
    },
    async waitForFreshChat(seedChatId) {
      calls.push(`fresh-chat:${seedChatId}`);
    },
    async waitForCodexSelectable() {
      calls.push("codex-selectable");
    },
    async selectCodex() {
      calls.push("select-codex");
    },
    async snapshot() {
      calls.push(`snapshot-${sent}`);
      return snapshot({
        currentSessionId: sent === 0 ? "" : "session-1",
        projectionRevision: String(sent + 1),
        turns: ECHOES.slice(0, sent).map((echo, index) =>
          completedTurn(index + 1, `Reply with exactly: ${echo}`, echo),
        ),
      });
    },
    async fillPrompt(prompt) {
      calls.push(`fill:${prompt}`);
    },
    async waitForSendReady() {
      calls.push("send-ready");
    },
    async send() {
      sent += 1;
      calls.push(`send-${sent}`);
    },
    async runtimeObservation() {
      calls.push("runtime-observation");
      return runtimeObservation();
    },
  };
  const result = await runAcp04ModeA({}, Date.now() + 5_000, ECHOES, {
    driver,
    codexVersion: CODEX_VERSION,
  });
  assert.equal(result.state.phase, "complete");
  assert.deepEqual(calls.slice(0, 9), [
    "editor-ready",
    "enter-agent",
    "seeded-chat",
    "seeded-chat-id",
    "new-chat",
    "fresh-chat:chat-seeded",
    "codex-selectable",
    "select-codex",
    "snapshot-0",
  ]);
  assert.equal(calls.filter((call) => call === "new-chat").length, 1);
});

test("clean Chat navigation accepts one top-level New chat and only an empty replacement Chat", () => {
  const dom = new JSDOM(
    `<!doctype html><main class="workbench workbench--agent"><div role="radiogroup" aria-label="Window mode"><input type="radio" name="workbench-window-mode" value="editor"><input type="radio" name="workbench-window-mode" value="agent" checked></div><aside class="project-sidebar" aria-label="Projects and chats"><div class="project-sidebar__actions"><button type="button" title="New chat">New chat</button></div><button class="project-chat" data-chat-id="chat-seeded" aria-current="page">Seeded chat</button></aside><section class="agent-chat-pane agent-chat-pane--focused"><section class="chat-surface" aria-label="Agent chat"></section></section><section class="agent-chat-pane"><section class="chat-surface" aria-label="Agent chat"></section></section></main>`,
  );
  const previous = { document: globalThis.document };
  globalThis.document = dom.window.document;
  try {
    const ui = createAcp04Ui(CODEX_VERSION);
    const action = dom.window.document.querySelector(
      '.project-sidebar__actions button[title="New chat"]',
    );
    let clicks = 0;
    action.addEventListener("click", () => {
      clicks += 1;
      const rail = dom.window.document.querySelector(".project-chat");
      rail.textContent = "Fresh chat";
      rail.dataset.chatId = "chat-fresh";
    });
    assert.equal(eval(ui.seededChatId), "chat-seeded");
    assert.equal(eval(freshChatReadyExpression("chat-seeded")), false);
    assert.equal(eval(ui.createFreshChat), true);
    assert.equal(clicks, 1);
    assert.equal(eval(freshChatReadyExpression("chat-seeded")), true);
    action.after(action.cloneNode(true));
    assert.equal(eval(ui.createFreshChat), false);
    assert.equal(clicks, 1);
    dom.window.document
      .querySelectorAll('.project-sidebar__actions button[title="New chat"]')
      .forEach((button) => button.remove());
    assert.equal(eval(ui.createFreshChat), false);
  } finally {
    globalThis.document = previous.document;
    dom.window.close();
  }
});

test("browser DOM contract selects the discovered Codex display and exposes exact two-turn ids", () => {
  const dom = new JSDOM(
    `<!doctype html><main class="workbench workbench--editor"><div role="radiogroup" aria-label="Window mode"><input type="radio" name="workbench-window-mode" value="editor" checked><input type="radio" name="workbench-window-mode" value="agent"></div><button class="project-chat" aria-current="page">Chat</button><section class="agent-chat-pane agent-chat-pane--focused"><section class="chat-surface" aria-label="Agent chat"><div class="composer__controls"><select><option value="codex">Codex \u2014 ${CODEX_VERSION}</option></select><select><option value="gpt-5.6" selected>gpt-5.6</option></select></div><div class="session-state">Ready</div><textarea id="prompt-chat-1" aria-label="Message agent"></textarea><section class="acp-chat-projection" aria-label="Agent chat ACP" data-project-id="project-1" data-chat-id="chat-1" data-current-session-id="session-1" data-projection-revision="3"><section class="acp-turn" data-turn-id="turn-1" data-session-id="session-1" data-runtime-epoch="7" data-turn-ordinal="1"><header><span data-turn-status="turn-1" role="status" aria-label="Completed"></span></header><article data-item-id="item-1-user" data-message-role="user"><span class="acp-markdown">Reply with exactly: SAGE_ACP04_A_TURN_1</span></article><article data-item-id="item-1-assistant" data-message-role="assistant"><span class="acp-markdown">SAGE_ACP04_A_TURN_1</span></article></section><section class="acp-turn" data-turn-id="turn-2" data-session-id="session-1" data-runtime-epoch="7" data-turn-ordinal="2"><header><span data-turn-status="turn-2" role="status" aria-label="Completed"></span></header><article data-item-id="item-2-user" data-message-role="user"><span class="acp-markdown">Reply with exactly: SAGE_ACP04_A_TURN_2</span></article><article data-item-id="item-2-assistant" data-message-role="assistant"><span class="acp-markdown">SAGE_ACP04_A_TURN_2</span></article></section></section></section></section></main>`,
  );
  const previous = {
    document: globalThis.document,
    HTMLSelectElement: globalThis.HTMLSelectElement,
    Event: globalThis.Event,
  };
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLSelectElement: dom.window.HTMLSelectElement,
    Event: dom.window.Event,
  });
  try {
    Object.defineProperty(dom.window.HTMLElement.prototype, "innerText", {
      configurable: true,
      get() {
        return this.textContent;
      },
    });
    const ui = createAcp04Ui(CODEX_VERSION);
    assert.equal(eval(ui.editorReady), true);
    assert.equal(eval(ui.enterAgentFromEditor), true);
    assert.equal(eval(ui.editorReady), false);
    dom.window.document.querySelector("main").className =
      "workbench workbench--agent";
    assert.equal(eval(ui.seededAgentReady), true);
    assert.equal(eval(ui.codexSelectable), true);
    assert.equal(eval(ui.selectCodex), true);
    assert.equal(eval(ui.codexReady), true);
    assert.equal(eval(ACP04_SNAPSHOT_EXPRESSION).composerEnabled, true);
    const composer = dom.window.document.querySelector(
      'textarea[aria-label="Message agent"]',
    );
    composer.disabled = true;
    assert.equal(eval(ui.codexReady), false);
    assert.equal(eval(ACP04_SNAPSHOT_EXPRESSION).composerEnabled, false);
    composer.disabled = false;
    composer.after(composer.cloneNode());
    assert.equal(eval(ui.codexReady), false);
    assert.equal(eval(ACP04_SNAPSHOT_EXPRESSION).composerEnabled, false);
    composer.nextElementSibling.remove();
    const codex = dom.window.document.querySelector("select option");
    let changes = 0;
    codex.parentElement.addEventListener("change", () => {
      changes += 1;
    });
    codex.disabled = true;
    assert.equal(eval(ui.codexSelectable), false);
    assert.equal(eval(ui.selectCodex), false);
    assert.equal(changes, 0);
    const actual = eval(ACP04_SNAPSHOT_EXPRESSION);
    assert.equal(actual.projectionRevision, "3");
    assert.deepEqual(
      actual.turns.map((turn) => turn.id),
      ["turn-1", "turn-2"],
    );
    assert.deepEqual(
      actual.turns.flatMap((turn) => turn.items.map((item) => item.id)),
      ["item-1-user", "item-1-assistant", "item-2-user", "item-2-assistant"],
    );
  } finally {
    Object.assign(globalThis, previous);
    dom.window.close();
  }
});

test("browser DOM contract refuses Agent, ambiguous, and absent Editor states without clicking", () => {
  const dom = new JSDOM(
    `<!doctype html><main class="workbench workbench--agent"><div role="radiogroup" aria-label="Window mode"><input type="radio" name="workbench-window-mode" value="editor"><input type="radio" name="workbench-window-mode" value="agent" checked></div></main>`,
  );
  const previous = { document: globalThis.document };
  globalThis.document = dom.window.document;
  try {
    const ui = createAcp04Ui(CODEX_VERSION);
    const editor = dom.window.document.querySelector('[value="editor"]');
    const agent = dom.window.document.querySelector('[value="agent"]');
    let clicks = 0;
    agent.addEventListener("click", () => {
      clicks += 1;
    });
    assert.equal(eval(ui.editorReady), false);
    assert.equal(eval(ui.enterAgentFromEditor), false);
    dom.window.document.querySelector("main").className =
      "workbench workbench--editor";
    editor.checked = true;
    agent.checked = false;
    dom.window.document.body.append(
      dom.window.document.querySelector("main").cloneNode(false),
    );
    assert.equal(eval(ui.editorReady), false);
    assert.equal(eval(ui.enterAgentFromEditor), false);
    dom.window.document
      .querySelectorAll("main")
      .forEach((main) => main.remove());
    assert.equal(eval(ui.editorReady), false);
    assert.equal(eval(ui.enterAgentFromEditor), false);
    assert.equal(clicks, 0);
  } finally {
    globalThis.document = previous.document;
    dom.window.close();
  }
});

test("browser DOM contract fills only one enabled dynamic agent composer", () => {
  const dom = new JSDOM(
    '<!doctype html><section class="agent-chat-pane agent-chat-pane--focused"><textarea id="prompt-chat-1" aria-label="Message agent"></textarea></section>',
  );
  const previous = {
    document: globalThis.document,
    HTMLTextAreaElement: globalThis.HTMLTextAreaElement,
    Event: globalThis.Event,
  };
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLTextAreaElement: dom.window.HTMLTextAreaElement,
    Event: dom.window.Event,
  });
  try {
    const composer = dom.window.document.querySelector(
      'textarea[aria-label="Message agent"]',
    );
    let inputs = 0;
    composer.addEventListener("input", () => {
      inputs += 1;
    });
    assert.equal(eval(fillPromptExpression("hello")), true);
    assert.equal(composer.value, "hello");
    assert.equal(inputs, 1);
    composer.disabled = true;
    assert.equal(eval(fillPromptExpression("disabled")), false);
    composer.disabled = false;
    composer.after(composer.cloneNode());
    assert.equal(eval(fillPromptExpression("ambiguous")), false);
    assert.equal(composer.value, "hello");
    assert.equal(inputs, 1);
  } finally {
    Object.assign(globalThis, previous);
    dom.window.close();
  }
});

test("receipt binds exact ordered artifact hashes and excludes ACP-04 acceptance", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-acp04-receipt-"));
  try {
    const profile = path.join(root, "profile");
    const profileArtifact = path.join(root, "acp04-profile-identity.json");
    mkdirSync(profile);
    writeFileSync(
      path.join(profile, ".sage-disposable-profile-v1"),
      "sage-disposable-profile-v1\n",
      { flag: "wx" },
    );
    writeFileSync(
      profileArtifact,
      JSON.stringify({
        schemaVersion: 1,
        profileRoot: profile,
        profileIdentitySha256: createHash("sha256")
          .update(profile.toLowerCase())
          .digest("hex"),
      }),
    );
    const transcript = path.join(root, "acp04-mode-a-observations.json");
    const screenshot = path.join(root, "acp04-mode-a-complete.png");
    writeFileSync(transcript, "observations\n");
    writeFileSync(screenshot, "png\n");
    const receipt = createAcp04Receipt(
      {
        ...parseAcp04ProbeArguments(argumentsFor()),
        output: root,
        profile: { path: profile, identityArtifact: profileArtifact },
      },
      {
        state: completeState(),
        startedAt: "2026-08-30T00:00:00.000Z",
        completedAt: "2026-08-30T00:00:01.000Z",
        runtimeObservation: runtimeObservation(),
      },
      { id: "target-1" },
      [transcript, screenshot],
      { connectionCount: 1, maxInFlight: 1, requestCount: 12 },
    );
    assert.deepEqual(Object.keys(receipt), [
      "schemaVersion",
      "completed",
      "evidenceClass",
      "sourceBinding",
      "mode",
      "excludedClaims",
      "source",
      "startedAt",
      "completedAt",
      "identities",
      "runtime",
      "cdp",
      "observation",
      "runtimeObservation",
      "artifacts",
    ]);
    assert.equal(receipt.sourceBinding, "collection-only");
    assert.ok(receipt.excludedClaims.includes("ACP-04 acceptance"));
    assert.deepEqual(
      receipt.artifacts.map((item) => item.name),
      ["acp04-mode-a-observations.json", "acp04-mode-a-complete.png"],
    );
    assert.equal(
      receipt.artifacts[0].sha256,
      createHash("sha256").update(readFileSync(transcript)).digest("hex"),
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("finalization closes CDP before identity revalidation and receipt write", async () => {
  const calls = [];
  await finalizeAcp04Collection({
    closeCdp: async () => calls.push("close-cdp"),
    revalidateIdentities: async () => calls.push("revalidate-identities"),
    createReceipt: () => {
      calls.push("create-receipt");
      return {};
    },
    writePendingReceipt: () => calls.push("write-pending"),
  });
  assert.deepEqual(calls, [
    "close-cdp",
    "revalidate-identities",
    "create-receipt",
    "write-pending",
  ]);
});

test("finalizer admits the exact PowerShell terminal-zero shape and rejects role substitution", () => {
  const encoded = (value) =>
    Buffer.from(JSON.stringify(value), "utf8").toString("base64");
  const sage = encoded({
    role: "sage",
    identityLost: false,
    remainingCount: 0,
  });
  const vite = encoded({
    role: "vite",
    identityLost: false,
    remainingCount: 0,
  });

  assert.deepEqual(decodeTerminalZeroCleanup(sage, vite), {
    sage: { role: "sage", identityLost: false, remainingCount: 0 },
    vite: { role: "vite", identityLost: false, remainingCount: 0 },
  });
  assert.throws(
    () => decodeTerminalZeroCleanup(vite, sage),
    /acp04-cleanup-invalid/,
  );
  assert.throws(
    () =>
      decodeTerminalZeroCleanup(
        encoded({
          role: "sage",
          identityLost: false,
          remainingCount: 0,
          extra: true,
        }),
        vite,
      ),
    /acp04-cleanup-invalid/,
  );
});

test("wrapper defaults to native IDE and exposes the bounded ACP route", () => {
  const wrapper = readFileSync(
    path.resolve("scripts/run-source-functional-native-ide-journey.ps1"),
    "utf8",
  );
  assert.match(wrapper, /JourneyKind = "native-ide"/u);
  assert.match(wrapper, /JourneyKind -eq "acp04-live"/u);
  assert.match(wrapper, /source-functional-acp04-live-probe\.mjs/u);
  assert.match(wrapper, /Publish-PendingJourneyReceipt/u);
});

test("native diagnostic attestation hashes bounded logs and rejects the exact failure pair", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-acp04-logs-"));
  try {
    const stdout = path.join(root, "sage.stdout.log");
    const stderr = path.join(root, "sage.stderr.log");
    writeFileSync(stdout, "Sage ready\n");
    writeFileSync(stderr, "ACP runner reached process-tree zero\n");
    const safe = attestNativeDiagnostics(stdout, stderr);
    assert.match(safe.stdout.sha256, /^[a-f0-9]{64}$/u);
    writeFileSync(
      stderr,
      'recovery_checkpoint="acp_runner.protocol_failure.pre_termination" protocol_failure_reason=ProtocolFailure\n',
    );
    assert.throws(
      () => attestNativeDiagnostics(stdout, stderr),
      /acp04-native-diagnostic-failure/,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("native diagnostic attestation rejects Closed, recovery, and capacity while allowing unrelated text", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-acp04-diagnostics-"));
  try {
    const stdout = path.join(root, "sage.stdout.log");
    const stderr = path.join(root, "sage.stderr.log");
    for (const line of [
      "host_error=Closed",
      "ACP recovery checkpoint reached",
      "ACP capacity refused",
    ]) {
      writeFileSync(stdout, "ordinary output\n");
      writeFileSync(stderr, `${line}\n`);
      assert.throws(
        () => attestNativeDiagnostics(stdout, stderr),
        /acp04-native-diagnostic-failure/,
      );
    }
    writeFileSync(stdout, "capacity planning is unrelated\n");
    writeFileSync(stderr, "normal application lifecycle\n");
    assert.doesNotThrow(() => attestNativeDiagnostics(stdout, stderr));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("runtime observation is owner-bound and root-process truth is independent from admitted launcher", () => {
  const observed = validateRuntimeObservation(runtimeObservation(), {
    projectId: "project-1",
    chatId: "chat-1",
    sessionId: "session-1",
    runtimeEpoch: "7",
  });
  assert.match(observed.admittedEntrypoint.canonicalPath, /codex\.cmd$/u);
  assert.match(observed.retainedRootProcess.canonicalImagePath, /node\.exe$/u);
  assert.notEqual(
    observed.admittedEntrypoint.canonicalPath,
    observed.retainedRootProcess.canonicalImagePath,
  );
});

function runtimeObservation(runtimeEpoch = "7") {
  return {
    owner: {
      projectId: "project-1",
      chatId: "chat-1",
      sessionId: "session-1",
      runtimeEpoch,
    },
    admittedEntrypoint: {
      canonicalPath: "C:\\Tools\\codex.cmd",
      sha256: SHA,
      bytes: 100,
      version: CODEX_VERSION,
      bindingDigest: SHA,
      kind: "windowsBatchLauncher",
    },
    retainedRootProcess: {
      processId: 9001,
      creationTimeFiletime100ns: "134000000000000000",
      canonicalImagePath: "C:\\Program Files\\nodejs\\node.exe",
      sha256: SHA,
      bytes: 200,
    },
  };
}

test("Codex cmd preflight waits for its captured-owner acknowledgement before starting Codex", async () => {
  let invocation;
  await revalidateCodexPreflight(
    {
      codex: {
        path: "C:\\Tools\\codex.cmd",
        version: CODEX_VERSION,
        semanticVersion: "0.146.0",
      },
      output: "C:\\Sage\\local-evidence\\ACP04-live-test",
    },
    Date.now() + 1_000,
    {
      validateCodex: () => {},
      runTool: async (...args) => {
        invocation = args;
        return `${CODEX_VERSION}\r\n`;
      },
    },
  );
  assert.match(invocation[0], /cmd\.exe$/iu);
  assert.deepEqual(invocation[1].slice(0, 3), ["/d", "/s", "/c"]);
  assert.equal(invocation[4].identityHandshake, "captured-owner\r\n");
  assert.match(invocation[4].expectedExecutablePath, /cmd\.exe$/iu);
  assert.equal(invocation[4].windowsVerbatimArguments, true);
  assert.equal(
    invocation[1][3],
    'set /p "SAGE_ACP04_ACK=" && call "C:\\Tools\\codex.cmd" --version',
  );
  assert.equal(invocation[4].commandRole, "codex-version-preflight");
  assert.equal(invocation[4].maxOutputBytes, 4096);
});

test("Codex preflight rejects command-expansion paths before spawning", async () => {
  for (const unsafe of [
    "%",
    "!",
    "^",
    "&",
    "|",
    "<",
    ">",
    "(",
    ")",
    "\r",
    "\n",
    "\0",
    '"',
  ]) {
    let spawned = false;
    const options = {
      ...parseAcp04ProbeArguments(
        argumentsFor({
          "codex-path": `C:\\Tools\\Codex${unsafe}CLI\\codex.cmd`,
        }),
      ),
    };
    await assert.rejects(
      () =>
        preflightCodex(options, Date.now() + 1_000, {
          runTool: async () => {
            spawned = true;
            return CODEX_VERSION;
          },
        }),
      /acp04-codex-preflight-invalid/,
    );
    assert.equal(
      spawned,
      false,
      `unsafe Codex path ${JSON.stringify(unsafe)} spawned`,
    );
  }
});

test("Codex preflight binds a valid spaced path before its acknowledgement-gated cmd command", async () => {
  const events = [];
  const options = {
    ...parseAcp04ProbeArguments(
      argumentsFor({ "codex-path": "C:\\Tools\\Codex CLI\\codex.cmd" }),
    ),
  };
  const observed = await preflightCodex(options, Date.now() + 1_000, {
    validateCodex(pathname, expected) {
      events.push("validate");
      assert.equal(pathname, "C:\\Tools\\Codex CLI\\codex.cmd");
      assert.deepEqual(expected, options.codex);
    },
    runTool: async (command, args, _deadline, _output, toolOptions) => {
      events.push("spawn");
      assert.match(command, /cmd\.exe$/iu);
      assert.deepEqual(args.slice(0, 3), ["/d", "/s", "/c"]);
      assert.equal(toolOptions.identityHandshake, "captured-owner\r\n");
      assert.match(toolOptions.expectedExecutablePath, /cmd\.exe$/iu);
      assert.equal(toolOptions.windowsVerbatimArguments, true);
      assert.equal(
        args[3],
        'set /p "SAGE_ACP04_ACK=" && call "C:\\Tools\\Codex CLI\\codex.cmd" --version',
      );
      return CODEX_VERSION;
    },
  });
  assert.deepEqual(events, ["validate", "spawn"]);
  assert.equal(observed.codex.version, CODEX_VERSION);
});

test("collector-owned Codex preflight records the exact bounded version", async () => {
  const options = parseAcp04ProbeArguments(argumentsFor());
  let invocation;
  const observed = await preflightCodex(options, Date.now() + 1_000, {
    command: "C:\\Windows\\System32\\cmd.exe",
    validateCodex: () => {},
    runTool: async (...args) => {
      invocation = args;
      return `${CODEX_VERSION}\r\n`;
    },
  });
  assert.equal(observed.codex.version, CODEX_VERSION);
  assert.equal(observed.codex.semanticVersion, "0.146.0");
  assert.deepEqual(invocation[1].slice(0, 3), ["/d", "/s", "/c"]);
  assert.equal(invocation[4].commandRole, "codex-version-preflight");
  assert.equal(invocation[4].maxOutputBytes, 4096);
});

test("pending validation rejects nested receipt mutation before finalization", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-acp04-pending-"));
  try {
    const profile = path.join(root, "profile");
    mkdirSync(profile);
    writeFileSync(
      path.join(profile, ".sage-disposable-profile-v1"),
      "sage-disposable-profile-v1\n",
    );
    const profileArtifact = path.join(root, "acp04-profile-identity.json");
    writeFileSync(
      profileArtifact,
      JSON.stringify({
        schemaVersion: 1,
        profileRoot: profile,
        profileIdentitySha256: createHash("sha256")
          .update(profile.toLowerCase())
          .digest("hex"),
      }),
    );
    const transcript = path.join(root, "acp04-mode-a-observations.json");
    const screenshot = path.join(root, "acp04-mode-a-complete.png");
    writeFileSync(transcript, "observations\n");
    writeFileSync(screenshot, "png\n");
    const options = {
      ...parseAcp04ProbeArguments(argumentsFor()),
      output: root,
      profile: { path: profile, identityArtifact: profileArtifact },
      codex: {
        ...parseAcp04ProbeArguments(argumentsFor()).codex,
        version: CODEX_VERSION,
        semanticVersion: "0.146.0",
      },
    };
    const candidate = createAcp04Receipt(
      options,
      {
        state: zeroBasedCompleteState(),
        startedAt: "2026-08-30T00:00:00.000Z",
        completedAt: "2026-08-30T00:00:01.000Z",
        runtimeObservation: runtimeObservation(),
      },
      { id: "target-1" },
      [transcript, screenshot],
      { connectionCount: 1, maxInFlight: 1, requestCount: 12 },
    );
    assert.doesNotThrow(() => validatePendingAcp04Candidate(candidate, root));
    assert.throws(
      () =>
        validatePendingAcp04Candidate(
          { ...candidate, cdp: { ...candidate.cdp, extra: true } },
          root,
        ),
      /acp04-pending-schema-invalid/,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("post-cleanup final receipt adds only hashes, cleanup proof, and finalization time", () => {
  const runtime = runtimeObservation();
  const candidate = {
    completed: true,
    evidenceClass: "source-functional-collection-only",
    sourceBinding: "collection-only",
    excludedClaims: ["ACP-04 acceptance"],
    observation: {
      projectIdSha256: createHash("sha256")
        .update(runtime.owner.projectId)
        .digest("hex"),
      chatIdSha256: createHash("sha256")
        .update(runtime.owner.chatId)
        .digest("hex"),
      sessionIdSha256: createHash("sha256")
        .update(runtime.owner.sessionId)
        .digest("hex"),
      runtimeEpoch: runtime.owner.runtimeEpoch,
    },
    runtimeObservation: runtime,
  };
  const diagnostics = {
    stdout: { bytes: 0, sha256: SHA },
    stderr: { bytes: 0, sha256: SHA },
  };
  const cleanup = {
    sage: { identityLost: false, remainingCount: 0 },
    vite: { identityLost: false, remainingCount: 0 },
  };
  const receipt = createFinalAcp04Receipt(candidate, diagnostics, cleanup);
  assert.deepEqual(receipt.nativeDiagnostics, diagnostics);
  assert.deepEqual(receipt.cleanup, cleanup);
  assert.equal(typeof receipt.finalizedAt, "string");
  assert.equal(JSON.stringify(receipt).includes("Sage ready"), false);
});

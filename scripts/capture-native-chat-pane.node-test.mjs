import assert from "node:assert/strict";
import test from "node:test";
import { JSDOM } from "jsdom";
import {
  NEW_TASK,
  clickExpression,
  openExpression,
  observationFor,
} from "./capture-native-chat-dom.mjs";
import { ownedPaneNodes, paneSelector } from "./capture-native-chat-pane.mjs";

test("New task stays document-scoped: it lives in Agent navigation, not a pane", () => {
  // paneSelector is serialized into page expressions, so it keeps a literal.
  assert.equal(paneSelector(NEW_TASK), NEW_TASK);
});

test("owner traversal rejects oversized result, control, and artifact collections before copying", () => {
  for (const kind of ["nodes", "controls", "roots"])
    assert.throws(
      () =>
        ownedPaneNodes(
          {
            querySelectorAll(selector) {
              if (kind === "nodes" && selector.endsWith(" .target"))
                return Array(65).fill({});
              if (kind === "controls" && selector.endsWith(" [aria-controls]"))
                return Array(65).fill({});
              if (kind === "roots" && selector === ".agent-artifact-pane")
                return Array(33).fill({ dataset: { ownerChatId: "fresh" } });
              return [];
            },
          },
          ".target",
          "fresh",
        ),
      /capture-owner-bound/u,
      kind,
    );
});

for (const panes of [1, 2]) {
  test(`capture controls and observations belong to the fresh chat in ${panes} panes`, async () => {
    const window = new JSDOM(
      `<section class="agent-chat-pane agent-chat-pane--focused"><button aria-label="Harness" data-selected-id="pi"></button><details><summary>Work</summary></details></section>${panes === 2 ? '<section class="agent-chat-pane"><button aria-label="Harness" data-selected-id="codex"></button><div class="acp-error">Other chat error</div></section>' : ""}`,
      { runScripts: "outside-only" },
    ).window;
    try {
      let clicks = 0;
      const own = window.document.querySelector(
        ".agent-chat-pane--focused button",
      );
      own.scrollIntoView = () => {};
      own.onclick = () => clicks++;
      const summary = window.document.querySelector("summary");
      summary.scrollIntoView = () => {};
      assert.equal(
        window.eval(clickExpression('button[aria-label="Harness"]')),
        true,
      );
      assert.equal(clicks, 1);
      assert.equal(window.eval(openExpression("details > summary")), true);
      window.workbench = {
        useWorkbenchStore: {
          getState: () => ({
            activeTabId: "diff-tab",
            project: { id: "project" },
            tabs: [{ id: "fresh", kind: "chat" }],
            harnesses: [{ id: "pi", bindingId: "pi-rpc" }],
          }),
        },
      };
      window.registry = {
        acpProjectionStoreFor: (chatId) => {
          assert.equal(chatId, "fresh");
          return {
            getState: () => ({
              page: {
                chatId: "fresh",
                projectId: "project",
                turns: [],
                sessions: [],
              },
            }),
          };
        },
      };
      window.owner = { pinnedAgentChatOwner: () => "fresh" };
      const expression = observationFor("pi-rpc")
        .replace("await import('/src/app/store.ts')", "workbench")
        .replace(
          "await import('/src/app/acpProjectionRegistry.ts')",
          "registry",
        )
        .replace("await import('/src/app/agentChatOwner.ts')", "owner");
      const observation = await window.eval(expression);
      assert.equal(observation.chatId, "fresh");
      assert.equal(observation.singlePane, panes === 1);
      assert.equal(observation.selected, "pi");
      assert.equal(observation.visible.error, false);
    } finally {
      window.close();
    }
  });
}

test("portalled picker controls are owned by the focused pane's aria-controls, not another chat", () => {
  const window = new JSDOM(
    '<section class="agent-chat-pane agent-chat-pane--focused"><button aria-label="Harness" aria-controls="own-menu"></button></section><section class="agent-chat-pane"><button aria-label="Harness" aria-controls="other-menu"></button></section><div id="own-menu"><button role="option" data-option-id="pi"></button></div><div id="other-menu"><button role="option" data-option-id="pi"></button></div>',
    { runScripts: "outside-only" },
  ).window;
  try {
    let clicks = 0;
    const option = window.document.querySelector("#own-menu button");
    option.scrollIntoView = () => {};
    option.onclick = () => clicks++;
    assert.equal(
      window.eval(clickExpression('[role="option"][data-option-id="pi"]')),
      true,
    );
    assert.equal(clicks, 1);
  } finally {
    window.close();
  }
});

test("resource observations include only the originating chat's artifact pane", () => {
  const window = new JSDOM(
    '<section class="agent-chat-pane agent-chat-pane--focused"></section><aside class="agent-artifact-pane" data-owner-chat-id="fresh"><button role="tab" data-resource-kind="diff"></button><section class="subagent-surface"></section></aside><aside class="agent-artifact-pane" data-owner-chat-id="other"><button role="tab" data-resource-kind="diff"></button></aside>',
  ).window;
  try {
    const nodes = ownedPaneNodes(
      window.document,
      '[role="tab"][data-resource-kind="diff"]',
      "fresh",
    );
    assert.equal(nodes.length, 1);
    assert.equal(nodes[0].parentElement.dataset.ownerChatId, "fresh");
    assert.equal(
      ownedPaneNodes(window.document, ".subagent-surface", "fresh").length,
      1,
    );
    assert.equal(
      ownedPaneNodes(
        window.document,
        '[role="tab"][data-resource-kind="diff"]',
        "missing",
      ).length,
      0,
    );
  } finally {
    window.close();
  }
});

test("picker observation includes the owning popup wrapper when aria-controls targets its nested listbox", () => {
  const window = new JSDOM(
    '<section class="agent-chat-pane agent-chat-pane--focused"><button aria-controls="own-listbox"></button></section><div class="menu-popover composer__harness-menu"><input aria-controls="own-listbox"><div id="own-listbox" role="listbox"></div></div><div class="menu-popover composer__harness-menu"><div id="other-listbox" role="listbox"></div></div>',
  ).window;
  try {
    const menus = ownedPaneNodes(window.document, ".composer__harness-menu");
    assert.equal(menus.length, 1);
    assert.equal(menus[0].querySelector('[role="listbox"]').id, "own-listbox");
  } finally {
    window.close();
  }
});

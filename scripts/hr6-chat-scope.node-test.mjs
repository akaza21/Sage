import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import { JSDOM } from "jsdom";

import {
  captureFocusedChatExpression,
  focusedChatSurfacePreamble,
  focusRememberedChatExpression,
  focusedRememberedChatSurfacePreamble,
  rememberedChatSurfacePreamble,
} from "./hr6-chat-scope.mjs";

function surfaceText(document) {
  return Function(
    "document",
    `return (()=>{${focusedChatSurfacePreamble("return null;")}return surface.textContent.trim()})()`,
  )(document);
}

function rememberedSurfaceText(document, window, name, focused) {
  const preamble = focused
    ? focusedRememberedChatSurfacePreamble(name, "return null;")
    : rememberedChatSurfacePreamble(name, "return null;");
  return Function(
    "document",
    "window",
    `return (()=>{${preamble}return surface.textContent.trim()})()`,
  )(document, window);
}

test("chat-local journey selectors use the focused pane after a second chat opens", () => {
  const { document } = new JSDOM(`
    <section class="agent-chat-pane"><section aria-label="Agent chat" class="chat-surface">Codex</section></section>
    <section class="agent-chat-pane agent-chat-pane--focused"><section aria-label="Agent chat" class="chat-surface">Claude</section></section>
  `).window;

  assert.equal(
    document.querySelector('section.chat-surface[aria-label="Agent chat"]')
      .textContent,
    "Codex",
  );
  assert.equal(surfaceText(document), "Claude");

  assert.equal(
    surfaceText(
      new JSDOM('<section class="agent-chat-pane"></section>').window.document,
    ),
    null,
  );
  assert.equal(
    surfaceText(
      new JSDOM(`
        <section class="agent-chat-pane agent-chat-pane--focused"><section aria-label="Agent chat" class="chat-surface">A</section></section>
        <section class="agent-chat-pane agent-chat-pane--focused"><section aria-label="Agent chat" class="chat-surface">B</section></section>
      `).window.document,
    ),
    null,
  );

  const journey = readFileSync(
    new URL("./hr6-live-run.mjs", import.meta.url),
    "utf8",
  );
  assert.match(journey, /focusedChatSurfacePreamble/);
  assert.doesNotMatch(
    journey,
    /document\.querySelector\('section\.chat-surface\[aria-label="Agent chat"\]'\)/,
  );
  assert.doesNotMatch(
    journey,
    /document\.querySelector\('\.searchable-picker__trigger\[aria-label="Harness"\]'\)/,
  );
  for (const selector of [
    'textarea[aria-label="Message agent"]',
    'button[aria-label="Send message"]',
    'button[aria-label="Cancel turn"]',
    "section.acp-turn",
    "details.acp-tool-rollup",
    ".session-state",
  ])
    assert.doesNotMatch(
      journey,
      new RegExp(
        `document\\.querySelector(?:All)?\\('${selector.replace(/[.[\]]/gu, "\\$&")}`,
      ),
    );
});

function focusRemembered(window, name) {
  return Function(
    "document",
    "window",
    `return (()=>{${focusRememberedChatExpression(name, "return false;")}})()`,
  )(window.document, window);
}

const PANES = `
  <section class="agent-chat-pane agent-chat-pane--focused" data-chat-id="chat-codex"><section aria-label="Agent chat" class="chat-surface">Codex<textarea aria-label="Message agent"></textarea></section></section>
  <section class="agent-chat-pane" data-chat-id="chat-claude"><section aria-label="Agent chat" class="chat-surface">Claude<textarea aria-label="Message agent"></textarea></section></section>
`;

test("a remembered Chat is focused through its pane's data-chat-id", () => {
  const dom = new JSDOM(PANES);
  const { document } = dom.window;
  const [codex, claude] = document.querySelectorAll(".agent-chat-pane");
  dom.window.__hr6Chats = { codex, claude };
  assert.equal(focusRemembered(dom.window, "claude"), true);
  assert.equal(
    document.activeElement,
    claude.querySelector('textarea[aria-label="Message agent"]'),
  );

  // Two panes claiming the same Chat, or a missing id, are ambiguous.
  codex.setAttribute("data-chat-id", "chat-claude");
  assert.equal(focusRemembered(dom.window, "claude"), false);
  codex.removeAttribute("data-chat-id");
  assert.equal(focusRemembered(dom.window, "codex"), false);
  assert.equal(focusRemembered(dom.window, "missing"), false);

  const journey = readFileSync(
    new URL("./hr6-live-run.mjs", import.meta.url),
    "utf8",
  );
  assert.match(journey, /focusRememberedChatExpression/);
  assert.doesNotMatch(journey, /agent-chat-pane__focus/);
});

test("recorded Chat scope cannot cross panes or accept focus ambiguity", () => {
  const dom = new JSDOM(PANES);
  const { document } = dom.window;
  const [codex, claude] = document.querySelectorAll(".agent-chat-pane");
  dom.window.__hr6Chats = { codex, claude };

  assert.equal(
    rememberedSurfaceText(document, dom.window, "codex", false),
    "Codex",
  );
  assert.equal(
    rememberedSurfaceText(document, dom.window, "codex", true),
    "Codex",
  );

  codex.classList.remove("agent-chat-pane--focused");
  claude.classList.add("agent-chat-pane--focused");
  assert.equal(
    rememberedSurfaceText(document, dom.window, "codex", true),
    null,
  );
  assert.equal(
    rememberedSurfaceText(document, dom.window, "claude", true),
    "Claude",
  );

  codex.classList.add("agent-chat-pane--focused");
  assert.equal(
    rememberedSurfaceText(document, dom.window, "claude", true),
    null,
  );
  assert.match(
    captureFocusedChatExpression("codex", "return false;"),
    /__hr6Chats/,
  );
});

test("a pane or surface hidden behind a scan overflow is never accepted as the only one", () => {
  const filler = "<span></span>".repeat(2047);
  assert.equal(
    surfaceText(
      new JSDOM(
        '<section class="agent-chat-pane agent-chat-pane--focused"><section aria-label="Agent chat" class="chat-surface">A</section></section>' +
          "<div>" +
          filler +
          "</div>" +
          '<section class="agent-chat-pane agent-chat-pane--focused"><section aria-label="Agent chat" class="chat-surface">B</section></section>',
      ).window.document,
    ),
    null,
  );
  assert.equal(
    surfaceText(
      new JSDOM(
        '<section class="agent-chat-pane agent-chat-pane--focused"><section aria-label="Agent chat" class="chat-surface">A</section><div>' +
          filler +
          '</div><section aria-label="Agent chat" class="chat-surface">B</section></section>',
      ).window.document,
    ),
    null,
  );
});

import { readFile } from "node:fs/promises";
import { createElement } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { AgentTaskStatus } from "../workbench/AgentTaskStatus";

afterEach(cleanup);

it("keeps status announcements compact and full harness errors expandable", async () => {
  const css = await readFile("src/styles/agent-task-header.css", "utf8");
  const compact = css.slice(css.indexOf("@container chat-center"));
  expect(compact).not.toMatch(/flex-wrap:\s*wrap|flex-basis:\s*100%/);
  render(
    createElement(AgentTaskStatus, {
      snapshot: null,
      harnessStatus: "Disconnected",
    }),
  );
  expect(screen.getByRole("status")).toHaveAccessibleName("Chat status: Idle");
  const explanation = screen.getByText("Disconnected", { selector: "p" });
  expect(explanation).not.toBeVisible();
  fireEvent.click(screen.getByLabelText("Harness status: Disconnected"));
  expect(explanation).toBeVisible();
  expect(css).not.toMatch(
    /font-size:\s*0;|text-overflow:\s*ellipsis;[^}]*harness/s,
  );
  expect(css).toMatch(/harness-status p\s*\{[^}]*white-space:\s*normal;/s);
  expect(css).toMatch(/harness-icon\s*\{[^}]*display:\s*block;/s);
});

it("omits neutral harness controls while keeping compact titles readable", async () => {
  const css = await readFile("src/styles/agent-task-header.css", "utf8");
  const compact = css.slice(
    css.indexOf("@container chat-center (max-width: 400px)"),
  );
  const notice = render(
    createElement(AgentTaskStatus, { snapshot: null, harnessStatus: "Ready" }),
  );
  expect(screen.queryByLabelText("Harness status: Ready")).toBeNull();
  notice.rerender(
    createElement(AgentTaskStatus, { snapshot: null, harnessStatus: "Live" }),
  );
  expect(screen.queryByLabelText("Harness status: Live")).toBeNull();
  expect(compact).toMatch(
    /\.agent-task-header__title\s*\{[^}]*min-width:\s*3em;/,
  );
});

it("keeps a failed rename readable without adding a compact header row", async () => {
  const css = await readFile("src/styles/agent-task-header.css", "utf8");
  const compact = css.slice(css.indexOf("@container chat-center"));
  expect(compact).toMatch(
    /\.agent-task-header__rename \[role="alert"\]\s*\{[^}]*position:\s*absolute;/s,
  );
  expect(compact).toMatch(
    /\.agent-task-header__rename \[role="alert"\]\s*\{[^}]*white-space:\s*normal;/s,
  );
  expect(compact).not.toMatch(
    /\.agent-task-header__rename \[role="alert"\]\s*\{[^}]*max-width:\s*32px;/s,
  );
});

it("keeps wrapped user bubbles rectangular with a bounded corner radius", async () => {
  const css = await readFile("src/features/chat/acp/acp-chat.css", "utf8");
  expect(css).toMatch(
    /\.acp-message--user\s*\{[^}]*border-radius:\s*var\(--radius-xl\)/s,
  );
  expect(css).not.toMatch(
    /\.acp-message--user\s*\{[^}]*border-radius:\s*var\(--radius-pill\)/s,
  );
});

it("uses a regular-weight home heading without a second project row", async () => {
  const css = await readFile("src/features/chat/chat-home.css", "utf8");
  expect(css).toMatch(
    /\.chat-home h3[^}]*font-weight:\s*var\(--font-weight-regular\)/s,
  );
  expect(css).not.toMatch(
    /\.chat-home \.project-scope\s*\{[^}]*margin-top:\s*var\(--space-3\)/s,
  );
});

it("styles Archived as a spaced, transparent sidebar disclosure", async () => {
  const css = await readFile(
    "src/styles/project-navigation-surface.css",
    "utf8",
  );
  const disclosure = css.match(
    /\.archived-chats__disclosure\s*\{([^}]+)\}/,
  )![1];
  expect(disclosure).toMatch(/display:\s*flex;/);
  expect(disclosure).toMatch(/align-items:\s*center;/);
  expect(disclosure).toMatch(/background:\s*transparent;/);
  expect(disclosure).toMatch(/border:\s*0;/);
  expect(disclosure).toMatch(/color:\s*var\(--color-text-secondary\)/);
});

it("gives the jump control a fixed round icon target", async () => {
  const css = await readFile("src/styles/chat-follow-end.css", "utf8");
  const jump = css.match(/\.chat-follow-end\s*\{([^}]+)\}/)![1];
  // Codex s2/f0001: a 28 px circle.
  expect(jump).toMatch(/width:\s*var\(--control-height-compact\)/);
  expect(jump).toMatch(/height:\s*var\(--control-height-compact\)/);
  expect(jump).toMatch(/border-radius:\s*var\(--radius-circle\)/);
});

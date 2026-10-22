import { readFile } from "node:fs/promises";
import { expect, it } from "vitest";

it("keeps compact chat headers on one line with bounded status text", async () => {
  const css = await readFile("src/styles/chat-container-controls.css", "utf8");
  const compact = css.slice(css.indexOf("@container chat-center"));
  expect(compact).not.toMatch(/flex-wrap:\s*wrap|flex-basis:\s*100%/);
  expect(compact).toMatch(/\.agent-task-header__status[^}]*font-size:\s*0;/s);
  expect(compact).toMatch(
    /\.agent-task-header__harness-status[^}]*text-overflow:\s*ellipsis;/s,
  );
});

it("keeps a failed rename readable without adding a compact header row", async () => {
  const css = await readFile("src/styles/chat-container-controls.css", "utf8");
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
  const css = await readFile(
    "src/features/chat/chat-transcript-polish.css",
    "utf8",
  );
  expect(css).toMatch(
    /\.message-list__content \.acp-message--user\s*\{[^}]*border-radius:\s*var\(--radius-xl\)/s,
  );
  expect(css).not.toMatch(
    /\.message-list__content \.acp-message--user\s*\{[^}]*border-radius:\s*var\(--radius-pill\)/s,
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
  expect(jump).toMatch(/width:\s*var\(--control-height-default\)/);
  expect(jump).toMatch(/height:\s*var\(--control-height-default\)/);
  expect(jump).toMatch(/border-radius:\s*var\(--radius-circle\)/);
});

import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Chat transcript polish stylesheet seam", () => {
  it("declares the turn trailing grid track and footer width/offset expressions without state selectors", async () => {
    const [transcriptCss, acpCss] = await Promise.all([
      readFile(resolve("src/features/chat/chat-transcript-polish.css"), "utf8"),
      readFile(resolve("src/features/chat/acp/acp-chat.css"), "utf8"),
    ]);

    expect(transcriptCss).toMatch(
      /\.message-list__content \.acp-turn\s*\{[^}]*grid-template-columns:\s*minmax\(0, 1fr\) 0;/s,
    );
    expect(acpCss).toMatch(
      /\.acp-turn\s*\{[^}]*column-gap:\s*var\(--space-3\);/s,
    );
    expect(transcriptCss).toMatch(
      /\.message-list__content \.acp-turn > \.acp-turn__header,\s*\.message-list__content \.acp-turn > \.acp-turn__items\s*\{[^}]*grid-column:\s*1;/s,
    );
    expect(transcriptCss).toMatch(
      /\.message-list__content:has\(> \.acp-chat-projection\) > \.chat-transcript-actions,\s*\.message-list__content > \.chat-fork-divider,\s*\.chat-fork-inherited > \.chat-transcript-actions\s*\{[^}]*width:\s*min\(calc\(100% - var\(--space-3\)\), var\(--reading-measure\)\);[^}]*margin-inline-start:\s*max\(\s*0px,\s*calc\(\(100% - var\(--space-3\) - var\(--reading-measure\)\) \/ 2\)\s*\);/s,
    );
  });

  it("declares shared reading-measure widths and auto margins for messages, work rows and diff cards", async () => {
    const [transcriptCss, diffCss] = await Promise.all([
      readFile(resolve("src/features/chat/chat-transcript-polish.css"), "utf8"),
      readFile(resolve("src/styles/changed-files-summary.css"), "utf8"),
    ]);

    expect(transcriptCss).toMatch(
      /\.message-list__content \.acp-message\s*\{[^}]*width:\s*min\(100%, var\(--reading-measure\)\);[^}]*margin-inline:\s*auto;/s,
    );
    expect(transcriptCss).toMatch(
      /\.message-list__content \.acp-turn__items\s*\{[^}]*width:\s*min\(100%, var\(--reading-measure\)\);[^}]*margin-inline:\s*auto;/s,
    );
    expect(transcriptCss).toMatch(
      /\.message-list__content \.acp-turn__items > \.chat-transcript-work\s*\{[^}]*width:\s*min\(100%, var\(--reading-measure\)\);[^}]*margin-inline:\s*auto;/s,
    );
    expect(diffCss).toMatch(
      /\.acp-file-changes\s*\{[^}]*width:\s*min\(100%, var\(--reading-measure\)\);[^}]*margin:\s*var\(--space-3\) auto 0;/s,
    );
  });

  it("declares muted activity tokens and interaction-only tool disclosure chevrons", async () => {
    const transcriptCss = await readFile(
      resolve("src/features/chat/chat-transcript-polish.css"),
      "utf8",
    );

    expect(transcriptCss).toMatch(
      /\.chat-transcript-work__groups \.acp-tool-rollup\s*\{[^}]*color:\s*color-mix\(\s*in srgb,\s*var\(--color-text-secondary\) 75%,\s*var\(--color-text-muted\)\s*\);[^}]*font-weight:\s*var\(--font-weight-regular\);/s,
    );
    expect(transcriptCss).toMatch(
      /\.chat-transcript-work__groups \.acp-tool-rollup__meta\s*\{\s*display:\s*contents;/s,
    );
    expect(transcriptCss).toMatch(
      /\.chat-transcript-work__groups \.acp-tool-rollup__chevron\s*\{[^}]*opacity:\s*0;/s,
    );
    expect(transcriptCss).toMatch(
      /\.chat-transcript-work__groups \.acp-tool-rollup > summary:hover[\s\S]*\.acp-tool-rollup__chevron[\s\S]*opacity:\s*1;/,
    );
    expect(transcriptCss).toMatch(
      /\.chat-transcript-work > summary,\s*\.chat-transcript-work__heading\s*\{[^}]*border-bottom:\s*1px solid var\(--color-border-subtle\);/s,
    );
    expect(transcriptCss).toMatch(
      /\.chat-transcript-work > summary\s*\{[^}]*cursor:\s*pointer;/s,
    );
    expect(transcriptCss).not.toMatch(/\.chat-transcript-work__heading::after/);
    expect(transcriptCss).toMatch(
      /\.chat-transcript-work__chevron\s*\{[^}]*display:\s*block;/s,
    );
    expect(transcriptCss).not.toMatch(
      /\.chat-transcript-work > summary:hover::after/,
    );
    expect(transcriptCss).not.toMatch(
      /\.chat-transcript-work > summary::before/,
    );
  });

  it("declares borderless assistant messages and capped rounded-rectangle user bubbles", async () => {
    const [css, tokens] = await Promise.all([
      readFile(resolve("src/features/chat/chat-transcript-polish.css"), "utf8"),
      readFile(resolve("src/styles/global.css"), "utf8"),
    ]);
    expect(css).toMatch(
      /\.message-list__content \.acp-message--assistant,[\s\S]*?\.message-list__content \.acp-message--system\s*\{[^}]*border:\s*0;[^}]*background:\s*transparent;/,
    );
    expect(css).toMatch(
      /\.message-list__content \.acp-message--user\s*\{[^}]*max-width:\s*min\(100%, 440px\);[^}]*border-radius:\s*var\(--radius-xl\);[^}]*background:\s*var\(--agent-shell-user-bubble\);/s,
    );
    expect(tokens).toMatch(/--radius-xl:\s*16px;/);
    expect(tokens).toMatch(/--agent-shell-user-bubble:\s*#2f2f2f;/);
  });

  it("declares hidden turn decorations and borderless tool/browser detail", async () => {
    const css = await readFile(
      resolve("src/features/chat/chat-transcript-polish.css"),
      "utf8",
    );
    expect(css).toMatch(
      /\.message-list__content \.acp-turn::before\s*\{[^}]*display:\s*none;/s,
    );
    expect(css).toMatch(
      /\.message-list__content \.acp-turn__state\s*\{[^}]*display:\s*none;/s,
    );
    expect(css).toMatch(
      /\.chat-transcript-work > summary,\s*\.chat-transcript-work__heading\s*\{[^}]*border-bottom:\s*1px solid var\(--color-border-subtle\);/s,
    );
    expect(css).toMatch(
      /\.chat-transcript-work__groups \.acp-tool-rollup__items \.acp-tool-card\s*\{[^}]*border:\s*0;[^}]*background:\s*transparent;/s,
    );
    expect(css).toMatch(
      /\.message-list__content \.acp-browser-operations \.acp-activity-card--browser\s*\{[^}]*border:\s*0;[^}]*background:\s*transparent;/s,
    );
  });
});

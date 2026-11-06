import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Chat transcript polish stylesheet seam", () => {
  it("declares a single turn track and lets the transcript column own its edges", async () => {
    const [transcriptCss, acpCss] = await Promise.all([
      readFile(resolve("src/features/chat/chat-transcript-polish.css"), "utf8"),
      readFile(resolve("src/features/chat/acp/acp-chat.css"), "utf8"),
    ]);

    expect(acpCss).toMatch(
      /\.acp-turn\s*\{[^}]*grid-template-columns:\s*minmax\(0, 1fr\);/s,
    );
    expect(acpCss).toMatch(
      /\.acp-turn__items\s*\{[^}]*grid-column:\s*1;[^}]*gap:\s*var\(--transcript-item-gap\);/s,
    );
    expect(acpCss).toMatch(/\.acp-turn__header\s*\{[^}]*grid-column:\s*1;/s);
    expect(transcriptCss).toMatch(
      /\.message-list__content\s*\{[^}]*width:\s*min\(100%, var\(--chat-column-measure\)\);[^}]*margin-inline:\s*auto;/s,
    );
  });

  it("declares a single reading measure while dock summaries inherit their slot", async () => {
    const [transcriptCss, diffCss] = await Promise.all([
      readFile(resolve("src/features/chat/chat-transcript-polish.css"), "utf8"),
      readFile(resolve("src/styles/changed-files-summary.css"), "utf8"),
    ]);
    expect(transcriptCss).toMatch(
      /\.message-list__content\s*\{[^}]*width:\s*min\(100%, var\(--chat-column-measure\)\);[^}]*margin-inline:\s*auto;/s,
    );
    expect(diffCss).toMatch(
      /\.acp-file-changes\s*\{[^}]*width:\s*var\(--edit-summary-width, min\(100%, var\(--reading-measure\)\)\);[^}]*margin:\s*var\(--edit-summary-margin, var\(--space-3\) auto 0\);/s,
    );
  });

  it("declares muted activity tokens and interaction-only tool disclosure chevrons", async () => {
    const transcriptCss = await readFile(
      resolve("src/features/chat/chat-transcript-polish.css"),
      "utf8",
    );
    const activityCss = await readFile(
      resolve("src/features/chat/acp/acp-activity-details.css"),
      "utf8",
    );

    expect(transcriptCss).toMatch(
      /\.chat-transcript-work__groups \.acp-tool-rollup\s*\{[^}]*color:\s*var\(--color-text-secondary\);[^}]*font-weight:\s*var\(--font-weight-regular\);/s,
    );
    expect(transcriptCss).toMatch(
      /\.chat-transcript-work__groups \.acp-tool-rollup__meta\s*\{\s*display:\s*contents;/s,
    );
    expect(activityCss).toMatch(
      /\.acp-tool-rollup__chevron\s*\{[^}]*opacity:\s*0;/s,
    );
    expect(activityCss).toMatch(
      /\.acp-tool-rollup > summary:hover[\s\S]*\.acp-tool-rollup__chevron[\s\S]*opacity:\s*1;/,
    );
    expect(transcriptCss).toMatch(
      /\.chat-transcript-work > summary,\s*\.chat-transcript-work__heading\s*\{[^}]*border-bottom:\s*var\(--border-width\) solid var\(--agent-shell-divider\);/s,
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
      readFile(resolve("src/features/chat/acp/acp-chat.css"), "utf8"),
      readFile(resolve("src/styles/global.css"), "utf8"),
    ]);
    expect(css).toMatch(
      /\.acp-message--assistant,[\s\S]*?\.acp-message--system\s*\{[^}]*border:\s*0;[^}]*background:\s*transparent;/,
    );
    expect(css).toMatch(
      /\.acp-message--user\s*\{[^}]*max-width:\s*70%;[^}]*border-radius:\s*var\(--radius-xl\);[^}]*background:\s*var\(--agent-shell-user-bubble\);/s,
    );
    expect(tokens).toMatch(/--radius-xl:\s*16px;/);
    expect(tokens).toMatch(/--color-sage:\s*#7fa877;/);
    expect(tokens).toMatch(
      /--agent-shell-user-bubble:\s*color-mix\(\s*in srgb,\s*var\(--color-sage\) 22%,\s*var\(--agent-shell-center\)\s*\);/,
    );
  });

  it("omits gutter decorations and declares borderless tool/browser detail", async () => {
    const css = (
      await Promise.all([
        readFile(
          resolve("src/features/chat/chat-transcript-polish.css"),
          "utf8",
        ),
        readFile(resolve("src/features/chat/acp/acp-chat.css"), "utf8"),
      ])
    ).join("\n");
    expect(css).not.toMatch(/\.acp-turn(?::|\[[^\]]+\]::)before/);
    expect(css).toMatch(/\.acp-turn__state\s*\{[^}]*display:\s*none;/s);
    expect(css).toMatch(
      /\.chat-transcript-work > summary,\s*\.chat-transcript-work__heading\s*\{[^}]*border-bottom:\s*var\(--border-width\) solid var\(--agent-shell-divider\);/s,
    );
    expect(css).toMatch(
      /\.chat-transcript-work__groups \.acp-tool-rollup__items \.acp-tool-card\s*\{[^}]*border:\s*0;[^}]*background:\s*transparent;/s,
    );
    expect(css).toMatch(
      /\.message-list__content \.acp-browser-operations \.acp-activity-card--browser\s*\{[^}]*border:\s*0;[^}]*background:\s*transparent;/s,
    );
  });
});

import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Chat transcript polish stylesheet seam", () => {
  it("centers assistant text, work rows, and diff cards on one reading measure", async () => {
    const [transcriptCss, diffCss] = await Promise.all([
      readFile(resolve("src/features/chat/chat-transcript-polish.css"), "utf8"),
      readFile(resolve("src/styles/changed-files-summary.css"), "utf8"),
    ]);

    expect(transcriptCss).toMatch(
      /\.message-list__content \.acp-message\s*\{[^}]*width:\s*min\(100%, var\(--reading-measure\)\);[^}]*margin-inline:\s*auto;/s,
    );
    expect(transcriptCss).toMatch(
      /\.message-list__content \.acp-turn__items\s*\{[^}]*width:\s*min\(100%, 660px\);[^}]*margin-inline:\s*auto;/s,
    );
    expect(transcriptCss).toMatch(
      /\.message-list__content \.acp-turn__items > \.chat-transcript-work\s*\{[^}]*width:\s*min\(100%, var\(--reading-measure\)\);[^}]*margin-inline:\s*auto;/s,
    );
    expect(diffCss).toMatch(
      /\.acp-file-changes\s*\{[^}]*width:\s*min\(100%, var\(--reading-measure\)\);[^}]*margin:\s*var\(--space-3\) auto 0;/s,
    );
  });

  it("keeps activity rows muted and reveals inline disclosure chevrons on interaction", async () => {
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
      /\.chat-transcript-work > summary\s*\{[^}]*border-bottom:\s*1px solid var\(--color-border-subtle\);/s,
    );
    expect(transcriptCss).toMatch(
      /\.chat-transcript-work > summary::after\s*\{[^}]*display:\s*none;/s,
    );
    expect(transcriptCss).toMatch(
      /\.chat-transcript-work > summary:hover::after,[\s\S]*\.chat-transcript-work > summary:focus-visible::after\s*\{[^}]*display:\s*block;/s,
    );
    expect(transcriptCss).not.toMatch(
      /\.chat-transcript-work > summary::before/,
    );
  });

  it("uses a plain assistant turn and a compact right-aligned user bubble", async () => {
    const [css, tokens] = await Promise.all([
      readFile(resolve("src/features/chat/chat-transcript-polish.css"), "utf8"),
      readFile(resolve("src/styles/global.css"), "utf8"),
    ]);
    expect(css).toMatch(
      /\.message-list__content \.acp-message--assistant,[\s\S]*?\.message-list__content \.acp-message--system\s*\{[^}]*border:\s*0;[^}]*background:\s*transparent;/,
    );
    expect(css).toMatch(
      /\.message-list__content \.acp-message--user\s*\{[^}]*max-width:\s*min\(100%, 440px\);[^}]*border-radius:\s*var\(--radius-pill\);[^}]*background:\s*var\(--agent-shell-user-bubble\);/s,
    );
    expect(tokens).toMatch(/--radius-pill:\s*999px;/);
    expect(tokens).toMatch(/--agent-shell-user-bubble:\s*#2f2f2f;/);
  });

  it("keeps live turn chrome and expanded tool detail unboxed", async () => {
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
      /\.chat-transcript-work > summary\s*\{[^}]*border-bottom:\s*1px solid var\(--color-border-subtle\);/s,
    );
    expect(css).toMatch(
      /\.chat-transcript-work__groups \.acp-tool-rollup__items \.acp-tool-card\s*\{[^}]*border:\s*0;[^}]*background:\s*transparent;/s,
    );
    expect(css).toMatch(
      /\.message-list__content \.acp-browser-operations \.acp-activity-card--browser\s*\{[^}]*border:\s*0;[^}]*background:\s*transparent;/s,
    );
  });
});

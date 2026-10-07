import { readFile, readdir } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

const css = (path: string) => readFile(resolve(path), "utf8");

describe("Agent motion style seams", () => {
  it("reveals Worked content by two pixels and brightens its summary without moving layout", async () => {
    const transcript = await css(
      "src/features/chat/chat-transcript-polish.css",
    );

    expect(transcript).toMatch(
      /\.chat-transcript-work\[open\] \.chat-transcript-work__groups\s*\{[^}]*animation:\s*sage-reveal-two-pixel var\(--motion-duration-normal\) ease-out both;/s,
    );
    expect(transcript).toMatch(
      /\.chat-transcript-work > summary\s*\{[^}]*transition:\s*color var\(--motion-duration-fast\) ease;/s,
    );
    expect(transcript).toMatch(
      /@media \(prefers-reduced-motion: reduce\)[\s\S]*\.chat-transcript-work\[open\] \.chat-transcript-work__groups[\s\S]*animation:\s*none;/,
    );
  });

  it("brightens activity rows, fades the hover chevron, and reveals Copy and Retry", async () => {
    const [transcript, activity] = await Promise.all([
      css("src/features/chat/chat-transcript-polish.css"),
      css("src/features/chat/acp/acp-activity-cards.css"),
    ]);

    expect(transcript).toMatch(
      /\.chat-transcript-activity > summary\s*\{[^}]*transition:\s*color var\(--motion-duration-fast\) ease;/s,
    );
    expect(transcript).toMatch(
      /\.chat-transcript-actions\s*\{[^}]*animation:\s*sage-reveal-two-pixel var\(--motion-duration-normal\) ease-out both;/s,
    );
    expect(activity).toMatch(
      /\.acp-tool-rollup > summary\s*\{[^}]*transition:\s*color var\(--motion-duration-fast\) ease;/s,
    );
    expect(transcript).toMatch(
      /\.chat-transcript-work__groups \.acp-tool-rollup__chevron\s*\{[^}]*transition:\s*opacity var\(--motion-duration-fast\) ease;/s,
    );
    expect(transcript).toMatch(
      /@media \(prefers-reduced-motion: reduce\)[\s\S]*\.chat-transcript-actions[\s\S]*animation:\s*none;/,
    );
  });

  it("reveals diff cards and Search Chats using the shared opacity and two-pixel motion", async () => {
    const [diff, dialogs, global] = await Promise.all([
      css("src/styles/changed-files-summary.css"),
      css("src/styles/shared-dialogs.css"),
      css("src/styles/global.css"),
    ]);

    expect(global).toMatch(
      /@keyframes sage-reveal-two-pixel\s*\{\s*from\s*\{\s*opacity:\s*0;\s*transform:\s*translateY\(2px\);\s*\}\s*to\s*\{\s*opacity:\s*1;\s*transform:\s*translateY\(0\);\s*\}\s*\}/,
    );
    expect(diff).toMatch(
      /\.acp-file-changes\s*\{[^}]*animation:\s*sage-reveal-two-pixel var\(--motion-duration-normal\) ease-out both;/s,
    );
    expect(dialogs).toMatch(
      /\.chat-search-dialog\[open\]\s*\{[^}]*animation:\s*sage-reveal-two-pixel var\(--motion-duration-normal\) ease-out both;/s,
    );
    expect(diff).toMatch(
      /@media \(prefers-reduced-motion: reduce\)[\s\S]*\.acp-file-changes[\s\S]*animation:\s*none;/,
    );
    expect(dialogs).toMatch(
      /@media \(prefers-reduced-motion: reduce\)[\s\S]*\.chat-search-dialog\[open\][\s\S]*animation:\s*none;/,
    );
  });

  it("uses duration tokens and animates no layout properties", async () => {
    const files = await Promise.all([
      css("src/features/chat/chat-transcript-polish.css"),
      css("src/features/chat/acp/acp-activity-cards.css"),
      css("src/styles/changed-files-summary.css"),
      css("src/styles/shared-dialogs.css"),
    ]);
    const global = await css("src/styles/global.css");

    expect(global).toMatch(/--motion-duration-fast:\s*100ms;/);
    expect(global).toMatch(/--motion-duration-normal:\s*160ms;/);
    expect(global).toMatch(
      /@media \(prefers-reduced-motion: reduce\)[\s\S]*--motion-duration-fast:\s*0ms;[\s\S]*--motion-duration-normal:\s*0ms;/,
    );
    for (const file of files) {
      expect(file).not.toMatch(/transition:[^;]*\b\d+ms\b/);
      expect(file).not.toMatch(
        /transition(?:-property)?:[^;]*(?:width|height|margin|padding|left|right|top|bottom|grid|flex|border-width)/,
      );
    }
  });

  it("tokens all activity pulses and the chat spinner, including reduced motion", async () => {
    const [global, activity, chat] = await Promise.all([
      css("src/styles/global.css"),
      css("src/features/chat/acp/acp-activity-cards.css"),
      css("src/styles/agent-chat-panes.css"),
    ]);
    expect(global).toMatch(/--motion-duration-pulse:\s*1\.4s;/);
    expect(global).toMatch(/--motion-duration-spin:\s*900ms;/);
    expect(global).toMatch(
      /@media \(prefers-reduced-motion: reduce\)[\s\S]*--motion-duration-pulse:\s*0ms;[\s\S]*--motion-duration-spin:\s*0ms;/,
    );
    expect(activity).not.toMatch(/animation:[^;]*\b(?:1\.4s|900ms)\b/);
    expect(activity).toContain(
      "sage-working-pulse var(--motion-duration-pulse)",
    );
    expect(chat).not.toMatch(/animation:[^;]*\b(?:1\.4s|900ms)\b/);
    expect(chat).toContain("agent-chat-chip-spin var(--motion-duration-spin)");
    const stylesheets = (await readdir("src", { recursive: true })).filter(
      (path) => path.endsWith(".css"),
    );
    for (const stylesheet of stylesheets) {
      const source = await css(`src/${stylesheet}`);
      expect(source, stylesheet).not.toMatch(
        /animation:[^;]*sage-working-pulse\s+1\.4s\b/,
      );
    }
  });
});

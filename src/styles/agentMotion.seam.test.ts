import { readFile, readdir } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

const css = (path: string) => readFile(resolve(path), "utf8");

describe("Agent motion style seams", () => {
  it("opens Worked content without a partial reveal and brightens its summary", async () => {
    const transcript = await css(
      "src/features/chat/chat-transcript-polish.css",
    );

    expect(transcript).not.toMatch(
      /\.chat-transcript-work\[open\] \.chat-transcript-work__groups\s*\{[^}]*animation:/s,
    );
    expect(transcript).toMatch(
      /\.chat-transcript-work > summary\s*\{[^}]*transition:\s*color var\(--motion-duration-fast\) ease;/s,
    );
    expect(transcript).toMatch(
      /@media \(prefers-reduced-motion: reduce\)[\s\S]*\.chat-transcript-actions[\s\S]*animation:\s*none;/,
    );
  });

  it("brightens activity rows, fades the hover chevron, and reveals Copy and Retry", async () => {
    const [transcript, activity] = await Promise.all([
      css("src/features/chat/chat-transcript-polish.css"),
      css("src/features/chat/acp/acp-activity-details.css"),
    ]);

    expect(transcript).toMatch(
      /\.chat-transcript-actions\s*\{[^}]*animation:\s*sage-reveal-two-pixel var\(--motion-duration-normal\) ease-out both;/s,
    );
    expect(activity).toMatch(
      /\.acp-tool-rollup > summary\s*\{[^}]*transition:\s*color var\(--motion-duration-fast\) ease;/s,
    );
    expect(transcript).toMatch(
      /\.chat-transcript-work__groups \.acp-tool-rollup__chevron\s*\{[^}]*transition:\s*opacity var\(--motion-duration-fast\) ease,\s*transform var\(--motion-duration-chevron\) ease-out;/s,
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
      css("src/features/chat/acp/acp-activity-details.css"),
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
    const [global, activity, detail, chat] = await Promise.all([
      css("src/styles/global.css"),
      css("src/features/chat/acp/acp-activity-cards.css"),
      css("src/features/chat/acp/acp-activity-details.css"),
      css("src/styles/agent-chat-panes.css"),
    ]);
    expect(global).toMatch(/--motion-duration-pulse:\s*1\.4s;/);
    expect(global).toMatch(/--motion-duration-spin:\s*900ms;/);
    expect(global).toMatch(
      /@media \(prefers-reduced-motion: reduce\)[\s\S]*--motion-duration-pulse:\s*0ms;[\s\S]*--motion-duration-spin:\s*0ms;/,
    );
    expect(activity).not.toMatch(/animation:[^;]*\b(?:1\.4s|900ms)\b/);
    expect(detail).not.toMatch(/animation:[^;]*\b(?:1\.4s|900ms)\b/);
    expect(activity).toContain(
      "sage-working-pulse var(--motion-duration-pulse)",
    );
    expect(chat).not.toMatch(/animation:[^;]*\b(?:1\.4s|900ms)\b/);
    expect(chat).toContain("sage-spin var(--motion-duration-spin)");
    expect(global).toMatch(/@keyframes sage-spin\s*\{/);
    const stylesheets = (await readdir("src", { recursive: true })).filter(
      (path) => path.endsWith(".css"),
    );
    for (const stylesheet of stylesheets) {
      const source = await css(`src/${stylesheet}`);
      expect(source, stylesheet).not.toMatch(
        /animation:[^;]*sage-working-pulse\s+1\.4s\b/,
      );
      // Durations come from --motion-* tokens so reduced motion zeroes them.
      expect(source, stylesheet).not.toMatch(
        /(?:animation|transition)(?:-duration|-delay)?:[^;{}]*(?<![\w.-])(?:\d*\.)?[1-9]\d*m?s\b/,
      );
      if (stylesheet.replaceAll("\\", "/") !== "styles/global.css") {
        expect(source, stylesheet).not.toMatch(/@keyframes [\w-]*spin\b/);
      }
    }
  });
});

/** Every `@media (condition)` block in a file, joined into one string. */
function blocks(source: string, condition: string) {
  const head = `@media (${condition})`;
  const found: string[] = [];
  for (
    let start = source.indexOf(head);
    start >= 0;
    start = source.indexOf(head, start + head.length)
  ) {
    let depth = 0;
    for (
      let index = source.indexOf("{", start);
      index < source.length;
      index++
    ) {
      if (source[index] === "{") depth++;
      if (source[index] === "}" && --depth === 0) {
        found.push(source.slice(start, index + 1));
        break;
      }
    }
  }
  return found.join("\n");
}

const fallbacks = ["prefers-reduced-motion: reduce", "forced-colors: active"];

describe("Codex live motion (parity spec section 10)", () => {
  it("sweeps a fixed 56 px white-core band left to right at 0.6 px/ms, then idles for the rest of a 4 s period (s8/f0174-f0177)", async () => {
    const [global, activity] = await Promise.all([
      css("src/styles/global.css"),
      css("src/features/chat/acp/acp-activity-details.css"),
    ]);
    expect(global).toMatch(/--motion-duration-shimmer:\s*4s;/);
    expect(global).toMatch(/--color-text-emphasis:\s*#fff;/);
    const rule = activity.match(/\.acp-live-shimmer\s*\{([\s\S]*?)\n\}/)![1];
    expect(rule).toMatch(
      /animation:\s*acp-live-shimmer var\(--motion-duration-shimmer\) linear\s+infinite;/,
    );
    // A fixed-pixel band (not a percentage of the label) with a white core
    // from 34% to 66% of 176 px: 56 px, between 60 px soft edges.
    expect(rule).toMatch(
      /transparent,\s*var\(--color-text-emphasis\) 34%,\s*var\(--color-text-emphasis\) 66%,\s*transparent\s*\)\s*-176px\s+0 \/ 176px 100% no-repeat,/,
    );
    expect(rule).not.toMatch(/300%/);
    const frames = activity.match(
      /@keyframes acp-live-shimmer\s*\{([\s\S]*?)\n\}/,
    )![1];
    // -176 px to 664 px is 840 px over 35% of 4 s (1400 ms): 0.6 px/ms,
    // entering at the left; the muted base layer never moves.
    expect(frames).toMatch(
      /0%\s*\{\s*background-position:\s*-176px 0,\s*0 0;\s*\}/,
    );
    expect(frames).toMatch(
      /35%,\s*100%\s*\{\s*background-position:\s*664px 0,\s*0 0;\s*\}/,
    );
    expect((664 + 176) / (0.35 * 4000)).toBeCloseTo(0.6, 2);
  });

  it("never shimmers the Working-for header", async () => {
    const transcript = await css(
      "src/features/chat/chat-transcript-polish.css",
    );
    expect(transcript).not.toMatch(
      /chat-transcript-work__heading[^{]*\{[^}]*animation/,
    );
  });

  it("fades only newly mounted streamed blocks, opacity-only, in 300 ms", async () => {
    const [global, markdown] = await Promise.all([
      css("src/styles/global.css"),
      css("src/features/chat/acp/chat-markdown.css"),
    ]);
    expect(global).toMatch(/--motion-duration-stream:\s*300ms;/);
    expect(markdown).toMatch(
      /\.acp-turn:is\(\s*\[data-turn-state="running"\],\s*\[data-turn-state\^="waiting"\]\s*\)\s*\.acp-message--assistant\s*\.acp-markdown\s*>\s*:not\(ul, ol, \.markdown-table\)/,
    );
    expect(markdown).toMatch(
      /animation:\s*acp-stream-in var\(--motion-duration-stream\) ease-out both;/,
    );
    expect(markdown).toMatch(
      /@keyframes acp-stream-in\s*\{\s*from\s*\{\s*opacity:\s*0\.15;\s*\}\s*\}/,
    );
    for (const condition of fallbacks)
      expect(blocks(markdown, condition), condition).toMatch(
        /\.acp-markdown[^{]*\{\s*animation:\s*none;/,
      );
  });

  it("staggers Worked and group rows top to bottom and rotates chevrons in 120 ms", async () => {
    const [global, transcript, activity] = await Promise.all([
      css("src/styles/global.css"),
      css("src/features/chat/chat-transcript-polish.css"),
      css("src/features/chat/acp/acp-activity-details.css"),
    ]);
    expect(global).toMatch(/--motion-duration-chevron:\s*120ms;/);
    expect(global).toMatch(/--motion-duration-row:\s*200ms;/);
    expect(global).toMatch(/--motion-stagger:\s*80ms;/);
    expect(global).toMatch(
      /@keyframes sage-row-in\s*\{\s*from\s*\{\s*opacity:\s*0\.3;\s*\}\s*\}/,
    );
    expect(transcript).toMatch(
      /\.chat-transcript-work\[open\]\s*\+\s*\.chat-transcript-work__groups\s*>\s*\*\s*\{[^}]*animation:\s*sage-row-in var\(--motion-duration-row\) ease-out both;[^}]*animation-delay:\s*calc\(var\(--sage-row, 5\) \* var\(--motion-stagger\)\);/s,
    );
    expect(activity).toMatch(
      /\.acp-tool-rollup__items > \*\s*\{[^}]*animation:\s*sage-row-in var\(--motion-duration-row\) ease-out both;[^}]*animation-delay:\s*calc\(var\(--sage-row, 5\) \* var\(--motion-stagger\)\);/s,
    );
    for (let row = 1; row <= 5; row++)
      expect(global).toMatch(
        new RegExp(
          `> :nth-child\\(${row}\\)\\s*\\{\\s*--sage-row:\\s*${row - 1};`,
        ),
      );
    expect(transcript).toMatch(
      /\.chat-transcript-work__chevron\s*\{[^}]*transition:\s*transform var\(--motion-duration-chevron\) ease-out;/s,
    );
    expect(activity).toMatch(
      /\.acp-tool-rollup__chevron\s*\{[^}]*transition:\s*transform var\(--motion-duration-chevron\) ease-out;/s,
    );
    // Collapse is instant: no rule animates a closed disclosure.
    expect(transcript).not.toMatch(/:not\(\[open\]\)[^{]*\{[^}]*animation/);
    for (const [source, selector] of [
      // Same selector as the motion rule, so the fallback wins on specificity.
      [
        transcript,
        "\\.chat-transcript-work\\[open\\]\\s*\\+\\s*\\.chat-transcript-work__groups\\s*>\\s*\\*",
      ],
      [activity, "\\.acp-tool-rollup__items > \\*"],
    ] as const)
      for (const condition of fallbacks)
        expect(blocks(source, condition), condition).toMatch(
          new RegExp(`${selector}[^{]*\\{\\s*animation:\\s*none;`),
        );
  });

  it("opens popovers opacity-only and closes mounted ones with a 50 ms exit", async () => {
    const [global, controls] = await Promise.all([
      css("src/styles/global.css"),
      css("src/styles/shared-controls.css"),
    ]);
    expect(global).toMatch(/--motion-duration-exit:\s*50ms;/);
    expect(controls).toMatch(
      /@keyframes menu-popover-enter\s*\{\s*from\s*\{\s*opacity:\s*0;\s*\}\s*\}/,
    );
    expect(controls).not.toMatch(/translateY\(-2px\)/);
    const popover = controls.match(/\.menu-popover\s*\{([^}]+)\}/)![1];
    expect(popover).toMatch(
      /transition:\s*opacity var\(--motion-duration-exit\) ease-in,\s*display var\(--motion-duration-exit\) allow-discrete;/,
    );
    expect(controls).toMatch(
      /\.menu-popover\[hidden\]\s*\{\s*display:\s*none;\s*opacity:\s*0;\s*pointer-events:\s*none;\s*\}/,
    );
    for (const condition of fallbacks)
      expect(blocks(controls, condition), condition).toMatch(
        /\.menu-popover\s*\{[^}]*animation:\s*none;[^}]*transition:\s*none;/,
      );
  });

  it("floats a 28 px jump control over the transcript column and fades it in 150 ms", async () => {
    const [global, follow, surface] = await Promise.all([
      css("src/styles/global.css"),
      css("src/styles/chat-follow-end.css"),
      css("src/styles/chat-surface.css"),
    ]);
    expect(global).toMatch(/--motion-duration-fade:\s*150ms;/);
    expect(surface).not.toMatch(/"follow"/);
    // It shares the transcript area instead of taking a grid row.
    const overlay = follow.match(
      /\.chat-surface--agent > \.chat-follow-end\s*\{([^}]+)\}/,
    )![1];
    expect(overlay).toMatch(/grid-area:\s*transcript;/);
    expect(overlay).toMatch(/align-self:\s*end;/);
    expect(overlay).toMatch(/justify-self:\s*center;/);
    // Centred on the reading column (s2/f0046): the scrollbar gutter and the
    // Turn's column gap sit right of it, and compact panes zero both.
    expect(overlay).toMatch(
      /margin:\s*0 var\(--chat-column-trailing\) var\(--space-6\) 0;/,
    );
    expect(surface).toMatch(
      /--chat-column-trailing:\s*var\(--scrollbar-size\);/,
    );
    const compact = await css("src/workbench/workbench-window-mode.css");
    expect(compact).not.toMatch(/--chat-column-trailing:/);
    // A white, thin 14 px arrow (s2/f0046); the activity dots keep theirs.
    expect(follow).toMatch(
      /\.chat-follow-end:not\(\[data-mode="activity"\]\) > svg\s*\{\s*color:\s*var\(--color-text-emphasis\);\s*stroke-width:\s*1;\s*\}/,
    );
    const jump = follow.match(/\.chat-follow-end\s*\{([^}]+)\}/)![1];
    expect(jump).toMatch(/width:\s*var\(--control-height-compact\);/);
    expect(jump).toMatch(/background:\s*var\(--color-canvas\);/);
    expect(jump).toMatch(
      /transition:\s*opacity var\(--motion-duration-fade\) ease-out,\s*display var\(--motion-duration-fade\) allow-discrete;/,
    );
    expect(follow).toMatch(
      /\.chat-follow-end\[hidden\]\s*\{\s*display:\s*none;\s*opacity:\s*0;\s*\}/,
    );
    expect(follow).toMatch(
      /@starting-style\s*\{\s*\.chat-follow-end\s*\{\s*opacity:\s*0;\s*\}\s*\}/,
    );
    for (const condition of fallbacks)
      expect(blocks(follow, condition), condition).toMatch(
        /\.chat-follow-end\s*\{[^}]*transition:\s*none;/,
      );
  });

  it("bounces the three new-activity dots left to right on a 1 s cycle", async () => {
    const [global, follow] = await Promise.all([
      css("src/styles/global.css"),
      css("src/styles/chat-follow-end.css"),
    ]);
    expect(global).toMatch(/--motion-duration-bounce:\s*1s;/);
    // The arrow becomes the middle dot; the button's pseudo-elements are the
    // outer two. A data-mode attribute, not the accessible name, selects it.
    const mode = '\\.chat-follow-end\\[data-mode="activity"\\]';
    expect(follow).not.toMatch(/aria-label/);
    const dots = `${mode}::before,\\s*${mode} > svg,\\s*${mode}::after`;
    expect(follow).toMatch(
      // s6/f0053: each dot is about 4 px on a 7-8 px pitch.
      new RegExp(
        `${dots}\\s*\\{[^}]*width:\\s*4px;[^}]*height:\\s*4px;[^}]*animation:\\s*sage-dot-bounce var\\(--motion-duration-bounce\\)\\s+ease-in-out\\s+infinite;`,
      ),
    );
    expect(follow).toMatch(
      new RegExp(`${mode}\\s*\\{[^}]*grid-auto-flow:\\s*column;`),
    );
    expect(follow).toMatch(
      new RegExp(
        `\\n${mode} > svg\\s*\\{\\s*animation-delay:\\s*calc\\(var\\(--motion-duration-bounce\\) \\/ 8\\);`,
      ),
    );
    expect(follow).toMatch(
      new RegExp(
        `\\n${mode}::after\\s*\\{\\s*animation-delay:\\s*calc\\(var\\(--motion-duration-bounce\\) \\/ 4\\);`,
      ),
    );
    const frames = follow.match(
      /@keyframes sage-dot-bounce\s*\{([\s\S]*?)\n\}/,
    )![1];
    expect(frames).toMatch(/25%\s*\{\s*transform:\s*translateY\(-2px\);/);
    for (const condition of fallbacks)
      expect(blocks(follow, condition), condition).toMatch(
        new RegExp(`${dots}\\s*\\{[^}]*animation:\\s*none;`),
      );
  });

  it("cross-fades the send fill on opacity in 150 ms and swaps the glyph instantly", async () => {
    const status = await css("src/styles/status-surfaces.css");
    const send = status.match(/\.send-button\s*\{([^}]+)\}/)![1];
    // No colour transition on the button itself; only the layer's opacity moves.
    expect(send).not.toMatch(/transition/);
    expect(send).toMatch(/background:\s*var\(--color-status-foreground\);/);
    expect(status).toMatch(
      /\.send-button::before\s*\{[^}]*background:\s*var\(--color-plane-raised\);[^}]*opacity:\s*0;[^}]*transition:\s*opacity var\(--motion-duration-fade\) ease-out;/s,
    );
    expect(status).toMatch(
      /\.send-button:disabled::before\s*\{[^}]*opacity:\s*1;/,
    );
    for (const condition of fallbacks)
      expect(blocks(status, condition), condition).toMatch(
        /\.send-button::before\s*\{[^}]*transition:\s*none;/,
      );
  });

  it("zeroes every new motion token under reduced motion", async () => {
    const global = await css("src/styles/global.css");
    const reduced = blocks(global, "prefers-reduced-motion: reduce");
    for (const token of [
      "shimmer",
      "stream",
      "chevron",
      "row",
      "exit",
      "fade",
      "bounce",
    ])
      expect(reduced, token).toMatch(
        new RegExp(`--motion-duration-${token}:\\s*0ms;`),
      );
    expect(reduced).toMatch(/--motion-stagger:\s*0ms;/);
  });

  it("animates only opacity and transform in the new motion keyframes", async () => {
    const sources = await Promise.all(
      [
        "src/styles/global.css",
        "src/styles/chat-follow-end.css",
        "src/styles/shared-controls.css",
        "src/features/chat/acp/chat-markdown.css",
      ].map(css),
    );
    let checked = 0;
    for (const source of sources)
      for (const [, name, body] of source.matchAll(
        /@keyframes ([\w-]+)\s*\{([\s\S]*?)\n\}/g,
      ))
        for (const [, property] of body.matchAll(/([\w-]+):/g)) {
          checked++;
          expect(["opacity", "transform"], `${name} ${property}`).toContain(
            property,
          );
        }
    expect(checked).toBeGreaterThan(6);
  });
});

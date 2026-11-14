import { readFile } from "node:fs/promises";
import { describe, expect, it } from "vitest";

const css = (path: string) =>
  readFile(new URL(path, import.meta.url), "utf8").then((text) =>
    text.replace(/\/\*[\s\S]*?\*\//g, ""),
  );

function rule(text: string, selector: string) {
  const escaped = selector
    .replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
    .replace(/\s+/g, "\\s*");
  const match = new RegExp(`(?:^|})\\s*${escaped}\\s*{([^}]*)}`).exec(text);
  return match?.[1] ?? null;
}

describe("Codex chrome parity (w10 group B)", () => {
  it("keeps the side tab's ✕ clear of the right-pane toggle (B1, s2/f0244)", async () => {
    const text = await css("../../workbench/workbench-window-mode.css");
    expect(
      rule(
        text,
        ".agent-chat-center__header--panes:has( > .agent-artifact-header-slot:not(:empty) ) > .agent-chat-center__side-tab",
      ),
    ).toMatch(/padding-inline-end:\s*40px;/);
    expect(
      rule(
        text,
        ".agent-chat-center__header--split > .agent-chat-center__side-tab",
      ),
    ).toMatch(/padding-inline-start:\s*var\(--space-1\);/);
  });

  it("shows an explicitly read-only model label at full strength (B2, s2/f0244)", async () => {
    const text = await css("../../styles/composer-model-picker.css");
    expect(
      rule(text, ".composer__model-trigger[data-readonly-label]:disabled"),
    ).toMatch(/opacity:\s*1;/);
    expect(
      rule(
        text,
        ".composer__model-group .composer__model-trigger:has(> .composer__model-effort)",
      ),
    ).toMatch(/max-width:\s*240px;/);
  });

  it("rings the sent chip in #3e3e3e at 30 px overall (B5, s2/f0149)", async () => {
    const [text, global] = await Promise.all([
      css("./transcript-interactions.css"),
      css("../../styles/global.css"),
    ]);
    const chip = rule(text, ".annotation-chip--sent .annotation-chip__trigger");
    const controls = await css("../../styles/shared-controls.css");
    expect(rule(controls, ".button--compact")).toMatch(
      /min-height:\s*var\(--control-height-compact\);/,
    );
    expect(global).toMatch(/--control-height-compact:\s*28px;/);
    expect(await css("./AnnotationChip.tsx")).toContain(
      'className="button--compact annotation-chip__trigger"',
    );
    expect(chip).toMatch(
      /outline:\s*1px solid var\(--chat-annotation-sent-edge\);/,
    );
    expect(global).toMatch(/--chat-annotation-sent-edge:\s*#3e3e3e;/);
  });

  it("gives in-chat dialogs one #2a2a2a surface and a hover-only option fill (B6, s2/f0055)", async () => {
    const [dialogs, fork, global] = await Promise.all([
      css("../../styles/shared-dialogs.css"),
      css("./fork-chat.css"),
      css("../../styles/global.css"),
    ]);
    expect(rule(dialogs, ".dialog--scrim")).toMatch(
      /background:\s*var\(--color-plane-raised\);/,
    );
    expect(global).toMatch(/--color-plane-raised:\s*#2a2a2a;/);
    expect(global).toMatch(/--chat-dialog-option-hover:\s*#353535;/);
    expect(
      rule(
        fork,
        ".fork-chat-dialog button:hover:not(:disabled), .fork-chat-dialog button:not([data-fresh]):focus-visible",
      ),
    ).toMatch(/var\(--chat-dialog-option-hover\)/);
  });

  it("draws the close dialog checkbox as an unfilled outline (B7, s2/f0352)", async () => {
    const [text, global] = await Promise.all([
      css("./transcript-interactions.css"),
      css("../../styles/global.css"),
    ]);
    const box = rule(text, '.close-side-chat-dialog input[type="checkbox"]');
    expect(box).toMatch(/appearance:\s*none;/);
    expect(box).toMatch(
      /border:\s*1px solid var\(--chat-dialog-checkbox-edge\);/,
    );
    expect(box).not.toMatch(/background/);
    expect(global).toMatch(/--chat-dialog-checkbox-edge:\s*#858585;/);
    expect(rule(text, ".close-side-chat-dialog")).toMatch(/font-size:\s*13px;/);
  });

  it("places Agent find below the header with a 32 px second row (B9, s6/f0676)", async () => {
    const text = await css("./chat-find.css");
    expect(rule(text, ".chat-surface--agent > .chat-find")).toMatch(
      /grid-area:\s*find;/,
    );
    const results = rule(text, ".chat-find__results");
    expect(results).toMatch(/height:\s*32px;/);
    expect(results).toMatch(/color:\s*var\(--agent-find-status\);/);
  });

  it("renders Deny in primary white (B10, s6/f0526)", async () => {
    const [text, global] = await Promise.all([
      css("./acp/permission-prompt.css"),
      css("../../styles/global.css"),
    ]);
    expect(rule(text, ".acp-permission-prompt__actions button")).toMatch(
      /color:\s*var\(--approval-primary-fill\);/,
    );
    expect(global).toMatch(/--approval-primary-fill:\s*#ffffff;/);
    expect(rule(text, ".acp-permission-prompt__actions kbd")).not.toMatch(
      /color:/,
    );
  });
});

import { describe, expect, it, vi } from "vitest";

import {
  composerDisabledReason,
  effectivePromptBytes,
  sizeReason,
} from "./composerDisabledReason";

const healthy = {
  actionBusy: false,
  activeTurn: false,
  availableBinding: true,
  canPrompt: true,
  chatLoadFailed: false,
  harnessAvailable: true,
  harnessDiscoveryPhase: "ready" as const,
  projectTransitioning: false,
  selectedHarness: true,
  useNativeAcp: true,
};

describe("composerDisabledReason", () => {
  it("allows sending when every precondition holds", () => {
    expect(composerDisabledReason(healthy)).toBeNull();
  });

  it("reports a failed chat load instead of blaming the harness", () => {
    const reason = composerDisabledReason({
      ...healthy,
      canPrompt: false,
      chatLoadFailed: true,
    });
    expect(reason).toBe("This chat failed to load. Retry to send a prompt.");
    expect(reason).not.toContain("Harness");
  });

  it("keeps blaming the harness only when the chat itself loaded", () => {
    expect(composerDisabledReason({ ...healthy, canPrompt: false })).toBe(
      "The selected harness cannot accept prompts in this session.",
    );
  });

  it("prefers a genuine harness fault over a failed chat load", () => {
    expect(
      composerDisabledReason({
        ...healthy,
        canPrompt: false,
        chatLoadFailed: true,
        harnessAvailable: false,
      }),
    ).toBe("The selected harness is unavailable.");
  });

  it("explains the disabled composer while a turn is already running", () => {
    expect(composerDisabledReason({ ...healthy, activeTurn: true })).toBe(
      "Wait for the active turn to finish, or cancel it when available.",
    );
  });

  it("blames the running turn rather than an unavailable harness", () => {
    expect(
      composerDisabledReason({
        ...healthy,
        activeTurn: true,
        canPrompt: false,
        chatLoadFailed: true,
        harnessAvailable: false,
      }),
    ).toBe("Wait for the active turn to finish, or cancel it when available.");
  });
});

describe("effectivePromptBytes", () => {
  const quote = {
    kind: "transcriptQuote" as const,
    ordinal: 1,
    sourceTurnId: "t",
    sourceItemId: "i",
    text: 'a "quote"',
    comment: "Why?",
    startOffset: 0,
    endOffset: 9,
  };
  it("is the prompt alone without context", () => {
    expect(effectivePromptBytes("Explain", [])).toBe(7);
  });
  it("never undercounts the Rust rendering of escaped quotes and comments", () => {
    const rendered =
      'Explain\n\nAnnotations on earlier assistant output (quoted context; cite as "Annotation N"):' +
      "\n- Annotation 1\n  Selected text (JSON-quoted): " +
      JSON.stringify(quote.text) +
      "\n  User comment (JSON-quoted): " +
      JSON.stringify(quote.comment);
    expect(effectivePromptBytes("Explain", [quote])).toBeGreaterThanOrEqual(
      new TextEncoder().encode(rendered).length,
    );
    expect(
      effectivePromptBytes("Explain", [{ ...quote, text: '"'.repeat(4000) }]),
    ).toBeGreaterThan(8000);
  });
});

it("measures a draft once until its text or context changes", () => {
  const contexts = [
    {
      kind: "transcriptQuote" as const,
      ordinal: 1,
      sourceTurnId: "t",
      sourceItemId: "i",
      text: "x".repeat(4000),
      startOffset: 0,
      endOffset: 4000,
    },
  ];
  const stringify = vi.spyOn(JSON, "stringify");
  expect(sizeReason("short", contexts)).toBeNull();
  const once = stringify.mock.calls.length;
  expect(sizeReason("short", contexts)).toBeNull();
  expect(stringify.mock.calls.length).toBe(once);
  expect(sizeReason("x".repeat(100_000), contexts)).toMatch(/too large/);
  expect(stringify.mock.calls.length).toBeGreaterThan(once);
  stringify.mockRestore();
});

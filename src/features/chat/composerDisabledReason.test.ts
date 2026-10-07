import { describe, expect, it } from "vitest";

import { composerDisabledReason } from "./composerDisabledReason";

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
    expect(reason).toBe("This Chat failed to load. Retry to send a prompt.");
    expect(reason).not.toContain("Harness");
  });

  it("keeps blaming the harness only when the chat itself loaded", () => {
    expect(composerDisabledReason({ ...healthy, canPrompt: false })).toBe(
      "The selected Harness cannot accept prompts in this session.",
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
    ).toBe("The selected Harness is unavailable.");
  });

  it("explains the disabled composer while a turn is already running", () => {
    expect(composerDisabledReason({ ...healthy, activeTurn: true })).toBe(
      "Wait for the active Turn to finish, or cancel it when available.",
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
    ).toBe("Wait for the active Turn to finish, or cancel it when available.");
  });
});

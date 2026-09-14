import { describe, expect, it } from "vitest";

import { safePreviewProjectionChanged } from "./acpDesktopPreview";

const owner = {
  projectId: "project-a",
  projectGeneration: "7",
  transitionEpoch: 7,
  chatId: "chat-a",
  sessionId: "session-a",
  runtimeEpoch: "3",
  previewRevision: "1",
  target: null,
  origin: null,
};

describe("Preview projection response admission", () => {
  it("retains a typed conflict on its native lifecycle phase", () => {
    expect(
      safePreviewProjectionChanged({
        kind: "preview",
        preview: { ...owner, phase: "starting", failure: "conflict" },
      }),
    ).toMatchObject({
      preview: { phase: "starting", failure: "conflict" },
    });
  });
});

import { describe, expect, it } from "vitest";

import type {
  PreviewFailure,
  PreviewPhase,
} from "../../domain/previewProjection";
import {
  previewFailureMessage,
  previewPhaseDetail,
  previewPhaseLabel,
} from "./previewPresentation";

describe("Preview presentation semantics", () => {
  it("keeps each starting phase legible instead of collapsing them together", () => {
    const phases: PreviewPhase[] = [
      "discovering",
      "starting",
      "waitingForOrigin",
      "healthChecking",
    ];

    expect(
      phases.map((phase) => ({
        detail: previewPhaseDetail(phase),
        label: previewPhaseLabel(phase),
      })),
    ).toEqual([
      "Finding runnable preview targets for this chat.",
      "Launching the selected Preview process.",
      "The process started. Waiting for its safe loopback address.",
      "The loopback server responded. Confirming it is ready.",
    ]);
  });

  it("maps every typed failure to distinct actionable detail", () => {
    const failures: PreviewFailure[] = [
      "scriptMissing",
      "originNotLoopback",
      "originUnsupported",
      "startupTimeout",
      "healthTimeout",
      "exitedEarly",
      "conflict",
      "internal",
    ];

    const details = failures.map(previewFailureMessage);
    expect(details).toEqual([
      "No runnable Preview script was found.",
      "Preview did not provide a safe loopback address.",
      "Preview supports http:// loopback origins only.",
      "Preview did not start before its deadline.",
      "Preview did not become healthy before its deadline.",
      "Preview exited before it became ready.",
      "Another Preview operation is already active.",
      "Preview failed safely.",
    ]);
    expect(new Set(details).size).toBe(failures.length);
  });
});

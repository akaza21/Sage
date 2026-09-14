import { describe, expect, it } from "vitest";

import type {
  PreviewFailure,
  PreviewPhase,
} from "../../domain/previewProjection";
import {
  previewFailureLabel,
  previewFailureMessage,
  previewPhaseDetail,
  previewPhaseLabel,
  previewPhaseTone,
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
        tone: previewPhaseTone(phase),
      })),
    ).toEqual([
      {
        detail: "Finding runnable Preview targets for this Chat.",
        label: "Discovering targets",
        tone: "starting",
      },
      {
        detail: "Launching the selected Preview process.",
        label: "Starting process",
        tone: "starting",
      },
      {
        detail: "The process started. Waiting for its safe loopback address.",
        label: "Waiting for origin",
        tone: "starting",
      },
      {
        detail: "The loopback server responded. Confirming it is ready.",
        label: "Checking health",
        tone: "starting",
      },
    ]);
  });

  it("maps every typed failure to distinct reason chrome and actionable detail", () => {
    const failures: PreviewFailure[] = [
      "scriptMissing",
      "originNotLoopback",
      "startupTimeout",
      "healthTimeout",
      "exitedEarly",
      "conflict",
      "internal",
    ];

    const presentations = failures.map((failure) => ({
      detail: previewFailureMessage(failure),
      label: previewFailureLabel(failure),
    }));
    expect(presentations).toEqual([
      {
        detail: "No runnable Preview script was found.",
        label: "Missing script",
      },
      {
        detail: "Preview did not provide a safe loopback address.",
        label: "Unsafe origin",
      },
      {
        detail: "Preview did not start before its deadline.",
        label: "Startup timeout",
      },
      {
        detail: "Preview did not become healthy before its deadline.",
        label: "Health timeout",
      },
      {
        detail: "Preview exited before it became ready.",
        label: "Early exit",
      },
      {
        detail: "Another Preview operation is already active.",
        label: "Ownership conflict",
      },
      { detail: "Preview failed safely.", label: "Internal error" },
    ]);
    expect(new Set(presentations.map(({ label }) => label)).size).toBe(
      failures.length,
    );
  });
});

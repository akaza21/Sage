import { describe, expect, it } from "vitest";

import { agentPreviewControllerOwner } from "./agentPreviewControllerOwner";
import type { HarnessBrowseOwner } from "../features/preview/harnessBrowse";
import type { AgentArtifactOwner } from "../domain/agentArtifact";

const browseOwner = {
  projectId: "project-a",
  projectGeneration: "7",
  transitionEpoch: 0,
  chatId: "chat:b",
  sessionId: "session-b",
  runtimeEpoch: "3",
} as HarnessBrowseOwner;

const focusedOwner = {
  projectId: "project-a",
  chatId: "chat:b",
  sessionId: "session-b",
} as unknown as AgentArtifactOwner;

const focusedPage = {
  sessions: [{ id: "session-b", runtimeEpoch: "3" }],
};

const focusedScope = { projectGeneration: "7" };

const alignedBrowseOwner = {
  projectId: "project-a",
  projectGeneration: "4",
  transitionEpoch: 4,
  chatId: "chat:b",
  sessionId: "session-b",
  runtimeEpoch: "3",
} as HarnessBrowseOwner;

const alignedScope = { projectGeneration: "4" };

describe("agentPreviewControllerOwner", () => {
  it("admits the owner when the generation and the transition epoch agree", () => {
    expect(
      agentPreviewControllerOwner({
        browseOwner: alignedBrowseOwner,
        focusedOwner,
        focusedPage,
        focusedScope: alignedScope,
        nativeTransitionEpoch: 4,
        projectGeneration: 4,
      }),
    ).toBe(alignedBrowseOwner);
  });

  it("withholds an owner whose generation and transition epoch disagree because discovery would reject it", () => {
    expect(
      agentPreviewControllerOwner({
        browseOwner,
        focusedOwner,
        focusedPage,
        focusedScope,
        nativeTransitionEpoch: 0,
        projectGeneration: 7,
      }),
    ).toBeNull();
  });

  it("withholds the owner when the workbench project generation moved past the browse owner", () => {
    expect(
      agentPreviewControllerOwner({
        browseOwner: alignedBrowseOwner,
        focusedOwner,
        focusedPage,
        focusedScope: alignedScope,
        nativeTransitionEpoch: 4,
        projectGeneration: 5,
      }),
    ).toBeNull();
  });

  it("withholds the owner when its transition epoch lags the native transition epoch", () => {
    expect(
      agentPreviewControllerOwner({
        browseOwner: { ...alignedBrowseOwner, transitionEpoch: 3 },
        focusedOwner,
        focusedPage,
        focusedScope: alignedScope,
        nativeTransitionEpoch: 4,
        projectGeneration: 4,
      }),
    ).toBeNull();
  });
});

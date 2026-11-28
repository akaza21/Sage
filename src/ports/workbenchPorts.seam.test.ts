import { describe, expect, it } from "vitest";

import {
  selectAgentSession,
  selectEditorDocument,
  selectOpenProject,
  selectRepositoryStatus,
  selectActiveChatId,
} from "../app/workbenchSelectors";
import type { WorkbenchState } from "../app/store";

describe("S0.3 workbench port seam", () => {
  it("exposes named store selectors so features do not expand aggregate store", () => {
    const state = {
      project: { id: "p", name: "P", roots: [] },
      sessionId: "session",
      selectedModelId: "sim-fast",
      repositories: { roots: [], truncated: false, elapsedMs: 0 },
      activeTabId: "chat:a",
      tabs: [
        {
          id: "chat:a",
          kind: "chat",
          title: "Chat",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeEditor: {
        phase: "ready",
        resource: { rootId: "r", relativePath: "a.ts" },
        document: {
          resource: { rootId: "r", relativePath: "a.ts" },
          base: { hash: "h", byteLength: 0, lineEnding: "lf" },
          lineEnding: "lf",
          content: "",
          recovered: false,
        },
        content: "",
        revision: 1,
        persistedRevision: 1,
        persisting: false,
        saving: false,
        message: null,
      },
    } as unknown as WorkbenchState;
    expect(selectOpenProject(state)?.id).toBe("p");
    expect(selectAgentSession(state)).toEqual({
      sessionId: "session",
      modelId: "sim-fast",
    });
    expect(selectRepositoryStatus(state).elapsedMs).toBe(0);
    expect(selectActiveChatId(state)).toBe("chat:a");
    expect(selectEditorDocument(state)?.resource.relativePath).toBe("a.ts");
  });
});

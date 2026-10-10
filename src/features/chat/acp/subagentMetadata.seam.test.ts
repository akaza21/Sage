import { describe, expect, it } from "vitest";
import { safeItem } from "../../../services/acpDesktopResponseItems";

function task(metadata: Record<string, unknown>) {
  return safeItem({
    id: "task",
    projectId: "project",
    chatId: "chat",
    sessionId: "session",
    turnId: "turn",
    runtimeEpoch: "1",
    ordinal: 1,
    origin: "primary",
    activityId: null,
    lifecycle: "completed",
    revision: "1",
    createdAt: 1,
    updatedAt: 2,
    terminalAt: 2,
    kind: "toolCall",
    toolState: "completed",
    payload: {
      inputSummary: null,
      presentation: {
        kind: "subagent",
        label: "Reviewer",
        activityId: null,
        ...metadata,
      },
    },
  });
}

describe("Subagent Task metadata admission", () => {
  it("preserves an explicitly supplied Task lifecycle hint without adding one to legacy tools", () => {
    const marked = task({ taskLifecycle: true });
    const legacy = task({});
    expect(marked).toMatchObject({
      payload: { presentation: { taskLifecycle: true } },
    });
    expect(legacy).not.toBeNull();
    if (legacy?.kind !== "toolCall") throw new Error("Expected legacy tool");
    expect(legacy.payload.presentation).toEqual({
      kind: "subagent",
      label: "Reviewer",
      activityId: null,
    });
  });

  it("rejects non-boolean Task lifecycle hints instead of granting lifecycle wording", () => {
    for (const taskLifecycle of ["true", 1, null, {}, []]) {
      expect(task({ taskLifecycle })).toBeNull();
    }
  });
});

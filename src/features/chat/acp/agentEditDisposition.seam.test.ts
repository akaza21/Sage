import { describe, expect, it, vi } from "vitest";

import { acpDesktop } from "../../../services/acpDesktop";
import * as agentFileOps from "./agentFileOps";
import { recordAgentEditDisposition } from "./agentEditDisposition";

describe("agent edit disposition seam", () => {
  it("accept_disposition_records_a_durable_checkpoint_for_the_turn_revision", async () => {
    const refreshed = { projection: { projectionRevision: "13" } } as never;
    const record = vi
      .spyOn(acpDesktop, "recordAgentEditDisposition")
      .mockResolvedValue(refreshed);
    const apply = vi.spyOn(agentFileOps, "applyAgentFileIntent");

    const result = await recordAgentEditDisposition({
      projectId: "project-1",
      chatId: "chat-1",
      sessionId: "session-1",
      turnId: "turn-1",
      expectedRevision: "9007199254740993",
      state: "accepted",
    } as never);

    expect(record).toHaveBeenCalledWith({
      projectId: "project-1",
      chatId: "chat-1",
      sessionId: "session-1",
      turnId: "turn-1",
      expectedRevision: "9007199254740993",
      state: "accepted",
    });
    expect(apply).not.toHaveBeenCalled();
    expect(result.projection.projectionRevision).toBe("13");
  });

  it("reject_disposition_does_not_mutate_workspace_files", async () => {
    const refreshed = { projection: { projectionRevision: "4" } } as never;
    const record = vi
      .spyOn(acpDesktop, "recordAgentEditDisposition")
      .mockResolvedValue(refreshed);
    const apply = vi.spyOn(agentFileOps, "applyAgentFileIntent");

    const result = await recordAgentEditDisposition({
      projectId: "project-1",
      chatId: "chat-1",
      sessionId: "session-1",
      turnId: "turn-1",
      expectedRevision: "3",
      state: "rejected",
    } as never);

    expect(record).toHaveBeenCalledOnce();
    expect(apply).not.toHaveBeenCalled();
    expect(result.projection.projectionRevision).toBe("4");
  });
});

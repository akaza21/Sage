import { describe, expect, it } from "vitest";
import { parseAgentDiffRef } from "../domain/agentDiff";
import { resourceTabUpdate } from "./resourceTabs";
import { useWorkbenchStore, type WorkbenchState } from "./store";

const resource = { rootId: "root", relativePath: "src/a.ts" };
function reference(turnId: string, snapshotRevision = "1") {
  return parseAgentDiffRef({
    projectId: "project",
    chatId: "chat",
    sessionId: "session",
    runtimeEpoch: "1",
    turnId,
    itemId: `item-${turnId}`,
    diffOrdinal: 0,
    snapshotRevision,
    ...resource,
    contentDigest: "a".repeat(64),
  })!;
}
function initial(): WorkbenchState {
  return { ...useWorkbenchStore.getState(), tabs: [], activeTabId: null };
}

describe("immutable snapshot resource tabs", () => {
  it("separates two Turns, evolving versions and ordinary Git for one file", () => {
    let state = initial();
    const refs = [reference("one"), reference("two"), reference("one", "2")];
    for (const ref of refs)
      state = {
        ...state,
        ...resourceTabUpdate(state, "diff", resource, undefined, ref),
      };
    state = { ...state, ...resourceTabUpdate(state, "diff", resource) };
    expect(state.tabs).toHaveLength(4);
    expect(new Set(state.tabs.map((tab) => tab.id)).size).toBe(4);
    expect(state.tabs.slice(0, 3).map((tab) => tab.diffRef)).toEqual(refs);
    expect(state.tabs[3].diffRef).toBeUndefined();
    const reopened = resourceTabUpdate(
      state,
      "diff",
      resource,
      undefined,
      refs[0],
    );
    expect(reopened.activeTabId).toBe(state.tabs[0].id);
    expect(reopened.tabs).toBeUndefined();
  });

  it("retains the shared resource-tab limit and never overwrites pinned tabs", () => {
    let state = initial();
    for (let turn = 0; turn < 32; turn++) {
      state = {
        ...state,
        ...resourceTabUpdate(
          state,
          "diff",
          resource,
          undefined,
          reference(`${turn}`),
        ),
      };
    }
    state.tabs = state.tabs.map((tab) => ({ ...tab, pinned: true }));
    const update = resourceTabUpdate(
      state,
      "diff",
      resource,
      undefined,
      reference("overflow"),
    );
    expect(update.error).toMatch(/32 resource-tab limit/);
    expect(update.tabs).toBeUndefined();
  });
});

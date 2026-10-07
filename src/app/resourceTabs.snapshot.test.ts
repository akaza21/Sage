import { describe, expect, it } from "vitest";
import { parseAgentDiffRef } from "../domain/agentDiff";
import { resourceTabUpdate } from "./resourceTabs";
import { assertBrowserProjectLayout } from "../services/browserLayout";
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
  it("accepts a browser tab in the durable project layout", () => {
    expect(() =>
      assertBrowserProjectLayout(
        {
          projectId: "project",
          tabs: [
            {
              id: "browser-1",
              kind: "browser",
              title: "Browser",
              position: 0,
              pinned: false,
              dirty: false,
            },
          ],
          activeTabId: "browser-1",
          selectedRootId: "root",
          selectedResource: null,
        },
        "project",
        [{ id: "root" }],
      ),
    ).not.toThrow();
  });

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

  it("round-trips two snapshot refs and one Git diff through durable layout validation", () => {
    let state = initial();
    const first = reference("one");
    const second = reference("one", "2");
    for (const ref of [first, second]) {
      state = {
        ...state,
        ...resourceTabUpdate(state, "diff", resource, undefined, ref),
      };
    }
    state = { ...state, ...resourceTabUpdate(state, "diff", resource) };
    expect(state.tabs).toHaveLength(3);
    expect(() =>
      assertBrowserProjectLayout(
        {
          projectId: "project",
          tabs: state.tabs,
          activeTabId: state.activeTabId,
          selectedRootId: "root",
          selectedResource: null,
        },
        "project",
        [{ id: "root" }],
      ),
    ).not.toThrow();
    const serialized = JSON.parse(JSON.stringify(state.tabs));
    expect(serialized[0].diffRef).toEqual(JSON.parse(JSON.stringify(first)));
    expect(serialized[1].diffRef).toEqual(JSON.parse(JSON.stringify(second)));
    expect(serialized[2].diffRef).toBeUndefined();
    expect(() =>
      assertBrowserProjectLayout(
        {
          projectId: "project",
          tabs: [
            ...state.tabs,
            {
              ...state.tabs[0],
              id: "diff:snapshot:bad-kind",
              kind: "file" as const,
            },
          ],
          activeTabId: state.activeTabId,
          selectedRootId: "root",
          selectedResource: null,
        },
        "project",
        [{ id: "root" }],
      ),
    ).toThrow();
  });
});

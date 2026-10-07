import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { agentDiffKey } from "../domain/agentDiffIdentity";
import type { AgentDiffSnapshot } from "../domain/agentDiff";
import { acpDesktop } from "../services/acpDesktop";
import { clearAcpProjectionRegistry } from "./acpProjectionRegistry";
import { runLoadDiff } from "./diffLoad";
import { resourceTabUpdate } from "./resourceTabs";
import { useWorkbenchStore } from "./store";
import {
  seedSnapshotOwner,
  snapshotRef as ref,
  snapshotResource as resource,
} from "./agentDiff.testFixture";

const initial = useWorkbenchStore.getState();
const get = useWorkbenchStore.getState;
const set = useWorkbenchStore.setState;
const key = agentDiffKey(ref);
let projection: ReturnType<typeof seedSnapshotOwner>;
const result = (): AgentDiffSnapshot => ({
  ...ref,
  projectionRevision: "1" as AgentDiffSnapshot["projectionRevision"],
  oldText: "old\n",
  newText: "new\n",
  oldBytes: 4,
  newBytes: 4,
});
beforeEach(() => {
  clearAcpProjectionRegistry();
  projection = seedSnapshotOwner();
  set(resourceTabUpdate(get(), "diff", resource, undefined, ref));
});
afterEach(() => {
  vi.restoreAllMocks();
  clearAcpProjectionRegistry();
  set(initial, true);
});
describe("snapshot loading through the real Diff dispatcher", () => {
  it("reads exact native metadata and retains only the bounded diff", async () => {
    const read = vi
      .spyOn(acpDesktop, "getFileSnapshot")
      .mockResolvedValue(result());
    await runLoadDiff(get, set, resource, undefined, ref);
    expect(read).toHaveBeenCalledExactlyOnceWith({
      ...ref,
      projectGeneration: "1",
      expectedProjectionRevision: "1",
    });
    expect(get().diffs[key]).toMatchObject({
      phase: "ready",
      value: {
        hunks: [
          {
            lines: [
              { kind: "removed", text: "old" },
              { kind: "added", text: "new" },
            ],
          },
        ],
      },
    });
    expect(JSON.stringify(get().diffs)).not.toContain("oldText");
    projection.setState({
      page: {
        ...projection.getState().page!,
        projectionRevision: "2" as AgentDiffSnapshot["projectionRevision"],
      },
    });
    await runLoadDiff(get, set, resource, undefined, ref);
    expect(read).toHaveBeenCalledOnce();
  });
  it.each([
    "generation",
    "session",
    "projection",
    "closed",
    "evicted",
    "reference",
  ])("discards a pending completion after %s changes", async (change) => {
    let complete!: (value: AgentDiffSnapshot) => void;
    const read = vi.spyOn(acpDesktop, "getFileSnapshot").mockImplementation(
      () =>
        new Promise((resolve) => {
          complete = resolve;
        }),
    );
    const loading = runLoadDiff(get, set, resource, undefined, ref);
    await vi.waitFor(() => expect(read).toHaveBeenCalledOnce());
    if (change === "generation") set({ projectGeneration: 2 });
    if (change === "session")
      projection.setState({
        page: {
          ...projection.getState().page!,
          currentSessionId: "different" as typeof ref.sessionId,
        },
      });
    if (change === "projection")
      projection.setState({
        page: {
          ...projection.getState().page!,
          projectionRevision: "2" as AgentDiffSnapshot["projectionRevision"],
        },
      });
    if (change === "closed") set({ tabs: [] });
    if (change === "evicted") set({ diffs: {} });
    if (change === "reference")
      set({
        tabs: get().tabs.map((tab) => ({
          ...tab,
          diffRef: { ...ref, snapshotRevision: "2" },
        })),
      });
    complete(result());
    await loading;
    expect(get().diffs[key]?.phase).not.toBe("ready");
    if (change === "session" || change === "projection")
      expect(get().diffs[key]?.phase).toBe("error");
    expect(read).toHaveBeenCalledOnce();
  });
  it("retains one shared 32-diff cache rather than a second snapshot pool", async () => {
    set({
      diffs: Object.fromEntries(
        Array.from({ length: 32 }, (_, index) => [
          `git-${index}`,
          {
            phase: "ready",
            value: { resource, hunks: [], binary: false, truncated: false },
          },
        ]),
      ),
    });
    vi.spyOn(acpDesktop, "getFileSnapshot").mockResolvedValue(result());
    await runLoadDiff(get, set, resource, undefined, ref);
    expect(Object.keys(get().diffs)).toHaveLength(32);
    expect(get().diffs["git-0"]).toBeUndefined();
    expect(get().diffs[key].phase).toBe("ready");
  });
  it("loads a second visible Chat's tab without requiring global active-tab ownership", async () => {
    set({ activeTabId: "some-other-visible-chat" });
    vi.spyOn(acpDesktop, "getFileSnapshot").mockResolvedValue(result());
    await runLoadDiff(get, set, resource, undefined, ref);
    expect(get().diffs[key].phase).toBe("ready");
  });
  it("carries the native transition epoch the bounded guard validates, not the local generation", async () => {
    set({ nativeTransitionEpoch: 4 });
    const read = vi
      .spyOn(acpDesktop, "getFileSnapshot")
      .mockResolvedValue(result());
    await runLoadDiff(get, set, resource, undefined, ref);
    expect(read).toHaveBeenCalledExactlyOnceWith({
      ...ref,
      projectGeneration: "4",
      expectedProjectionRevision: "1",
    });
    expect(get().diffs[key].phase).toBe("ready");
  });
  it("closing and reopening the same reference cannot revive a pre-close request", async () => {
    let complete!: (value: AgentDiffSnapshot) => void;
    const read = vi
      .spyOn(acpDesktop, "getFileSnapshot")
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            complete = resolve;
          }),
      )
      .mockResolvedValue(result());
    const first = runLoadDiff(get, set, resource, undefined, ref);
    await vi.waitFor(() => expect(read).toHaveBeenCalledOnce());
    get().closeTab(get().tabs[0].id);
    expect(get().diffs[key]).toBeUndefined();
    set(resourceTabUpdate(get(), "diff", resource, undefined, ref));
    await runLoadDiff(get, set, resource, undefined, ref);
    complete({ ...result(), newText: "stale\n" });
    await first;
    expect(read).toHaveBeenCalledTimes(2);
    expect(JSON.stringify(get().diffs)).not.toContain("stale");
    expect(get().diffs[key].phase).toBe("ready");
  });
  it("does not automatically reread after a stale-projection response; explicit retry succeeds", async () => {
    let complete!: (value: AgentDiffSnapshot) => void;
    const read = vi
      .spyOn(acpDesktop, "getFileSnapshot")
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            complete = resolve;
          }),
      )
      .mockResolvedValue({
        ...result(),
        projectionRevision: "2" as AgentDiffSnapshot["projectionRevision"],
      });
    const first = runLoadDiff(get, set, resource, undefined, ref);
    await vi.waitFor(() => expect(read).toHaveBeenCalledOnce());
    projection.setState({
      page: {
        ...projection.getState().page!,
        projectionRevision: "2" as AgentDiffSnapshot["projectionRevision"],
      },
    });
    complete(result());
    await first;
    expect(read).toHaveBeenCalledOnce();
    expect(get().diffs[key].phase).toBe("error");
    await runLoadDiff(get, set, resource, undefined, ref);
    expect(read).toHaveBeenLastCalledWith({
      ...ref,
      projectGeneration: "1",
      expectedProjectionRevision: "2",
    });
    expect(get().diffs[key].phase).toBe("ready");
  });
});

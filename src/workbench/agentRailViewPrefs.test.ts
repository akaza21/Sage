import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import {
  agentRailViewPrefs,
  resetAgentRailViewPrefs,
  restoreAgentRailViewPrefs,
  setAgentRailOrdering,
  setAgentRailOrganize,
  useAgentRailViewPrefs,
} from "./agentRailViewPrefs";

// A stand-in for the Rust settings store: it keeps one record and can refuse
// writes, so the tests observe what survives rather than which calls ran.
const settings = vi.hoisted(() => ({
  stored: null as null | { organize: string; ordering: string },
  refuseWrites: false,
}));
vi.mock("../ports/nativeInvoke", async (importOriginal) => ({
  ...((await importOriginal()) as object),
  invokeNative: async (
    command: string,
    args?: { view: { organize: string; ordering: string } },
  ) => {
    if (command === "agent_rail_view")
      return settings.stored ?? { organize: "project", ordering: "recency" };
    if (command === "agent_rail_set_view") {
      if (settings.refuseWrites) throw new Error("disk full");
      if (!args) throw new Error("missing view");
      settings.stored = { ...args.view };
      return settings.stored;
    }
    throw new Error(`unexpected command ${command}`);
  },
}));

beforeEach(() => {
  Object.assign(window, { __TAURI_INTERNALS__: {} });
  settings.stored = null;
  settings.refuseWrites = false;
  resetAgentRailViewPrefs();
});

afterEach(() => {
  delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  resetAgentRailViewPrefs();
});

it("restores the stored layout when the rail first reads it", async () => {
  settings.stored = { organize: "list", ordering: "manual" };
  const { result } = renderHook(() => useAgentRailViewPrefs());
  await waitFor(() =>
    expect(result.current).toMatchObject({
      organize: "list",
      ordering: "manual",
    }),
  );
});

it("stores each choice natively so a later launch gets it back", async () => {
  await restoreAgentRailViewPrefs();
  setAgentRailOrganize("list");
  setAgentRailOrdering("manual");
  await waitFor(() =>
    expect(settings.stored).toEqual({ organize: "list", ordering: "manual" }),
  );

  resetAgentRailViewPrefs();
  expect(agentRailViewPrefs().organize).toBe("project");
  await restoreAgentRailViewPrefs();
  expect(agentRailViewPrefs()).toMatchObject({
    organize: "list",
    ordering: "manual",
  });
});

it("returns to the stored layout when a write is refused", async () => {
  settings.stored = { organize: "project", ordering: "manual" };
  await restoreAgentRailViewPrefs();
  settings.refuseWrites = true;
  act(() => setAgentRailOrganize("list"));
  expect(agentRailViewPrefs().organize).toBe("list");
  await waitFor(() => expect(agentRailViewPrefs().organize).toBe("project"));
  expect(agentRailViewPrefs().ordering).toBe("manual");
});

it("ignores stored values the rail does not know", async () => {
  settings.stored = { organize: "status", ordering: "title" };
  await restoreAgentRailViewPrefs();
  expect(agentRailViewPrefs()).toMatchObject({
    organize: "project",
    ordering: "recency",
  });
});

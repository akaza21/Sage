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
  // When set, the first read waits until the test releases it.
  slowRead: null as null | Promise<void>,
  refuseReads: false,
}));
vi.mock("../ports/nativeInvoke", async (importOriginal) => ({
  ...((await importOriginal()) as object),
  invokeNative: async (
    command: string,
    args?: { view: { organize: string; ordering: string } },
  ) => {
    if (command === "agent_rail_view") {
      const stored = settings.stored ?? {
        organize: "project",
        ordering: "recency",
      };
      if (settings.slowRead) await settings.slowRead;
      if (settings.refuseReads) throw new Error("database locked");
      return stored;
    }
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
  settings.slowRead = null;
  settings.refuseReads = false;
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

it("keeps a choice made while the stored layout is still loading", async () => {
  settings.stored = { organize: "project", ordering: "manual" };
  let release = () => {};
  settings.slowRead = new Promise((resolve) => {
    release = resolve;
  });
  const loading = restoreAgentRailViewPrefs();
  act(() => setAgentRailOrganize("list"));
  expect(agentRailViewPrefs().organize).toBe("list");

  release();
  await act(() => loading);
  // The pick survives the late read, and the untouched sort is restored.
  expect(agentRailViewPrefs()).toMatchObject({
    organize: "list",
    ordering: "manual",
  });
  await waitFor(() =>
    expect(settings.stored).toEqual({ organize: "list", ordering: "manual" }),
  );
});

it("stores a choice made during a load that then fails", async () => {
  settings.stored = { organize: "project", ordering: "manual" };
  settings.refuseReads = true;
  let release = () => {};
  settings.slowRead = new Promise((resolve) => {
    release = resolve;
  });
  const loading = restoreAgentRailViewPrefs();
  act(() => setAgentRailOrganize("list"));
  release();
  await act(() => loading);
  expect(agentRailViewPrefs().organize).toBe("list");
  // The choice reaches the store with defaults for what could not be read.
  await waitFor(() =>
    expect(settings.stored).toEqual({ organize: "list", ordering: "recency" }),
  );
});

it("tries the read again after a failure with no pending choice", async () => {
  settings.stored = { organize: "list", ordering: "manual" };
  settings.refuseReads = true;
  await restoreAgentRailViewPrefs();
  expect(agentRailViewPrefs().organize).toBe("project");
  settings.refuseReads = false;
  await restoreAgentRailViewPrefs();
  expect(agentRailViewPrefs()).toMatchObject({
    organize: "list",
    ordering: "manual",
  });
});

it("loads the stored layout once storage recovers from a failed read and save", async () => {
  settings.stored = { organize: "project", ordering: "manual" };
  settings.refuseReads = true;
  settings.refuseWrites = true;
  let release = () => {};
  settings.slowRead = new Promise((resolve) => {
    release = resolve;
  });
  const loading = restoreAgentRailViewPrefs();
  act(() => setAgentRailOrganize("list"));
  release();
  await act(() => loading);
  // Neither the read nor the save landed, so the rail shows the defaults.
  await waitFor(() => expect(agentRailViewPrefs().organize).toBe("project"));

  settings.refuseReads = false;
  settings.refuseWrites = false;
  settings.slowRead = null;
  await restoreAgentRailViewPrefs();
  expect(agentRailViewPrefs()).toMatchObject({
    organize: "project",
    ordering: "manual",
  });
});

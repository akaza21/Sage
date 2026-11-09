import { afterEach, beforeEach, expect, it, vi } from "vitest";

import type { WorkbenchTab } from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { useWorkbenchStore } from "./store";

// The archive actions are a lazy chunk; the test decides when it arrives.
const chunk = vi.hoisted(() => {
  let release!: () => void;
  const arrived = new Promise<void>((resolve) => {
    release = resolve;
  });
  return { arrived, release };
});
vi.mock("./chatArchiveActions", async (importOriginal) => {
  await chunk.arrived;
  return importOriginal();
});

const tab = (id: string, position: number): WorkbenchTab => ({
  id,
  kind: "chat",
  title: id,
  position,
  pinned: false,
  dirty: false,
});
const project = (id: string) => ({
  id,
  name: id,
  roots: [
    {
      id: `${id}-root`,
      name: id,
      ordinal: 0,
      availability: "available" as const,
      message: null,
    },
  ],
});
const initial = useWorkbenchStore.getState();

const railChat = (id: string, position: number) => ({
  id,
  title: id,
  pinned: false,
  position,
});
// Switching Project takes a new projection generation.
const switchToProjectB = () =>
  useWorkbenchStore.setState((state) => ({
    project: project("project-b"),
    projectGeneration: state.projectGeneration + 1,
    tabs: [tab("chat:b1", 0)],
    activeTabId: "chat:b1",
  }));

beforeEach(() => {
  useWorkbenchStore.setState({
    phase: "ready",
    project: project("project-a"),
    projects: [
      {
        id: "project-x",
        name: "X",
        rootCount: 1,
        lastOpenedAt: 1,
        railChats: [railChat("chat:x1", 0), railChat("chat:x2", 1)],
      },
    ],
    tabs: [tab("chat:a1", 0), tab("chat:a2", 1)],
    activeTabId: "chat:a1",
    archivedChats: [],
    projectTransitioning: false,
    error: null,
  });
});
afterEach(() => {
  useWorkbenchStore.setState(initial, true);
  vi.restoreAllMocks();
});

it("drops an archive started before a Project switch that finished while its actions loaded", async () => {
  const archive = vi.spyOn(getWorkbenchPorts().chat, "archiveChat");
  const archiving = useWorkbenchStore.getState().archiveChat("chat:a1");
  // The next Project has one attached Chat, which could not be archived.
  switchToProjectB();
  chunk.release();

  await expect(archiving).resolves.toBe(false);
  expect(archive).not.toHaveBeenCalled();
  expect(useWorkbenchStore.getState().error).toBeNull();
  expect(useWorkbenchStore.getState().chatRowError).toBeNull();
  expect(useWorkbenchStore.getState().tabs).toEqual([tab("chat:b1", 0)]);
});

it("still runs an archive that names its Project after a Project switch", async () => {
  const archive = vi
    .spyOn(getWorkbenchPorts().chat, "archiveChat")
    .mockRejectedValue(new Error("refused by the test"));
  // A rail row archives a Chat of another Project by naming it.
  const archiving = useWorkbenchStore
    .getState()
    .archiveChat("chat:x1", "project-x", "row");
  switchToProjectB();
  chunk.release();

  await archiving;
  expect(archive).toHaveBeenCalledTimes(1);
  expect(archive.mock.calls[0][0]).toMatchObject({
    projectId: "project-x",
    chatId: "chat:x1",
  });
});

it("still reports the last attached Chat in the Project it was started in", async () => {
  chunk.release();
  useWorkbenchStore.setState({ tabs: [tab("chat:a1", 0)] });

  await expect(
    useWorkbenchStore.getState().archiveChat("chat:a1"),
  ).resolves.toBe(false);
  expect(useWorkbenchStore.getState().error).toBe(
    "The last attached Chat cannot be archived",
  );
});

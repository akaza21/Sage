import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { decodeSessionSurface } from "../../../services/acpSessionSurface";
import { SessionSurfacePanel } from "./SessionSurfacePanel";

const run = vi.fn();
vi.mock("./useSessionControl", () => ({
  useSessionControl: () => ({ status: { kind: "idle" }, run }),
}));

beforeEach(() => run.mockReset());
afterEach(cleanup);

const KEY = "0123456789abcdef01234567";
const TURN = "turn-6f9619ff-8b86-4011-b42d-00cf4fc964ff";

it("decodes only a bounded, well-formed surface", () => {
  const surface = decodeSessionSurface({
    activities: [
      {
        key: KEY,
        kind: "task",
        label: "npm test",
        state: "running",
        progress: "Using Bash",
        stoppable: true,
      },
    ],
    undoTurn: TURN,
  });
  expect(surface?.activities[0].stoppable).toBe(true);
  expect(surface?.undoTurn).toBe(TURN);
  expect(
    decodeSessionSurface({ activities: [], undoTurn: "x" })?.undoTurn,
  ).toBeUndefined();
  expect(
    decodeSessionSurface({
      activities: [
        { key: "task:raw-id", label: "x", kind: "task", state: "running" },
      ],
    }),
  ).toBeUndefined();
  expect(decodeSessionSurface({ activities: "x" })).toBeUndefined();
});

it("stops a running activity by its key", () => {
  render(
    <SessionSurfacePanel
      chatId="chat-a"
      surface={{
        activities: [
          {
            key: KEY,
            kind: "task",
            label: "Dev server",
            state: "running",
            progress: null,
            stoppable: true,
          },
          {
            key: KEY.replace("0", "f"),
            kind: "task",
            label: "Lint",
            state: "completed",
            progress: null,
            stoppable: false,
          },
        ],
      }}
    />,
  );
  const stops = screen.getAllByRole("button", { name: "Stop" });
  expect(stops).toHaveLength(1);
  fireEvent.click(stops[0]);
  expect(run).toHaveBeenCalledWith(
    { kind: "stopActivity", activityKey: KEY },
    "Stopping Dev server",
  );
});

it("undoes the latest turn after a confirming click", () => {
  render(
    <SessionSurfacePanel
      chatId="chat-one"
      surface={{ activities: [], undoTurn: TURN }}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Undo last turn" }));
  expect(run).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Confirm undo" }));
  expect(run).toHaveBeenCalledWith(
    { kind: "undoTurn", turnId: TURN },
    "Changes undone",
  );
  cleanup();
  render(
    <SessionSurfacePanel chatId="chat-one" surface={{ activities: [] }} />,
  );
  expect(screen.queryByRole("button", { name: /Undo/ })).toBeNull();
});

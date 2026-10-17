import { beforeEach, expect, it } from "vitest";
import { useSessionAccessModeStore } from "./sessionAccessModeStore";
import { resetWorkbenchPortsForTests } from "../ports/workbenchRuntime";
import { sessionAccessModeAvailability } from "../domain/sessionAccessMode";

beforeEach(async () => {
  resetWorkbenchPortsForTests();
  await useSessionAccessModeStore.getState().restore();
});

it("keeps rapid choices for different bindings isolated and persists both", async () => {
  const store = useSessionAccessModeStore.getState();
  store.setMode("codex-app-server", "workspaceWrite");
  store.setMode("opencode-acp", "readOnly");
  await store.settle();
  await store.restore();
  expect(useSessionAccessModeStore.getState().modes).toEqual({
    "codex-app-server": "workspaceWrite",
    "opencode-acp": "readOnly",
  });
});

it("defaults an unselected binding to its native safest mode without reusing another binding choice", () => {
  const ask = [
    {
      id: "ask" as const,
      label: "Ask each time",
      description: "Review permission requests.",
      fallback: true,
    },
  ];
  expect(sessionAccessModeAvailability(undefined, ask).effective).toBe("ask");
  expect(sessionAccessModeAvailability("readOnly", ask).effective).toBeNull();
});

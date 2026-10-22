import { createElement } from "react";
import { createRoot } from "react-dom/client";
import { acpProjectionStoreFor } from "../../../src/app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../../src/app/store";
import { installVisualScenario } from "./visualScenarioShell";
import { installVisualComposerAccessFixture } from "./surfaces/composerAccessVisualFixture";
import { installVisualComposerActionsFixture } from "./surfaces/composerActionsVisualFixture";
import { installVisualComposerEffortFixture } from "./surfaces/composerEffortVisualFixture";
import { installPopoverSubagents } from "./popoverSubagentsFixture";
import { NestedPopoverFixture } from "./NestedPopoverFixture";

const popoverWorkbench = {
  install: installVisualScenario,
  access: installVisualComposerAccessFixture,
  actions: installVisualComposerActionsFixture,
  effort: installVisualComposerEffortFixture,
  subagents: installPopoverSubagents,
  nested: () => {
    const composer = document.querySelector("form.composer");
    if (!composer) throw new Error("Nested fixture requires a composer");
    const host = document.createElement("span");
    composer.append(host);
    createRoot(host).render(createElement(NestedPopoverFixture));
  },
  settled: () =>
    useWorkbenchStore
      .getState()
      .tabs.filter((tab) => tab.kind === "chat")
      .every((tab) => {
        const state = acpProjectionStoreFor(tab.id).getState();
        return state.requestSequence > 0 && state.activeLoad === null;
      }),
};
Object.assign(window, { popoverWorkbench });
declare global {
  interface Window {
    popoverWorkbench: typeof popoverWorkbench;
  }
}

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createRef } from "react";
import { ComposerModelControls } from "./ComposerModelControls";
import { ComposerQueuedPrompts } from "./ComposerQueuedPrompts";
import { ChatHarnessControls } from "./ChatHarnessControls";
import { useSessionAccessModeStore } from "../../app/sessionAccessModeStore";

const initialAccess = useSessionAccessModeStore.getState();
beforeEach(() =>
  useSessionAccessModeStore.setState({ ready: true, mode: "readOnly" }),
);
afterEach(() => {
  cleanup();
  useSessionAccessModeStore.setState(initialAccess, true);
});

describe("Compact chat controls", () => {
  it("opens access options from a shield, returns Escape to that opener, and toggles closed on a second pointer click", () => {
    render(
      <ChatHarnessControls
        availableBinding
        canPrompt
        discoveryPhase="ready"
        modelLabelScope="compact"
        sessionHarnessId={null}
        sessionModelId={null}
        selectedHarnessId={null}
        selectedModelId={null}
        projectName={null}
        turnBusy={false}
        sessionConfig={null}
        permissionRequests={false}
        modePending={false}
        modeError={null}
        focused={false}
        onSetSessionMode={() => Promise.resolve()}
        onSetSessionConfigOption={() => Promise.resolve()}
        onSelectHarness={() => Promise.resolve()}
        onSelectModel={() => {}}
        onModeRemoved={() => {}}
      />,
    );
    const trigger = screen.getByRole("button", { name: "Access options" });
    expect(trigger).toHaveAttribute("aria-description", "Read-only");
    expect(trigger.querySelector('svg[aria-hidden="true"]')).not.toBeNull();
    fireEvent.click(trigger);
    const dialog = screen.getByRole("dialog", {
      name: "Composer options menu",
    });
    expect(dialog.parentElement).toBe(document.body);
    expect(
      screen.getByRole("button", { name: "Permission mode" }),
    ).toHaveFocus();
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(trigger).toHaveFocus();
    fireEvent.click(trigger);
    fireEvent.pointerDown(trigger);
    fireEvent.click(trigger);
    expect(
      screen.queryByRole("dialog", { name: "Composer options menu" }),
    ).toBeNull();
  });

  it("gives the model trigger a meaningful icon and the confirmed identity as its description", () => {
    render(
      <ComposerModelControls
        triggerRef={createRef()}
        panelId="models"
        open={false}
        onToggle={() => {}}
        onDismiss={() => {}}
        choices={[{ id: "model", name: "Advertised long model" }]}
        selectedId="model"
        modelLabel="Advertised long model"
        effortLabel={null}
        contextWindow={null}
        effortOption={undefined}
        disabled={false}
        pending={false}
        onSelectModel={() => {}}
        onSetOption={() => Promise.resolve()}
      />,
    );
    const trigger = screen.getByRole("button", { name: "Model" });
    expect(trigger).toHaveAttribute(
      "aria-description",
      "Advertised long model",
    );
    expect(trigger.querySelector(".composer__model-icon")).not.toBeNull();
  });

  it("opens queue actions outside the scrolling queue and returns focus on Escape", () => {
    render(
      <ComposerQueuedPrompts
        prompts={[
          {
            id: "queued",
            text: "Long queued prompt",
            attachedContext: [],
            bindingId: "binding",
            modelId: null,
            status: "blocked",
          },
        ]}
        disabled={false}
      />,
    );
    const trigger = screen.getByRole("button", {
      name: "More options for queued message: Long queued prompt",
    });
    fireEvent.click(trigger);
    const menu = screen.getByRole("menu", { name: "Queued message options" });
    expect(menu.parentElement).toBe(document.body);
    expect(
      screen.getByRole("menuitem", { name: "Retry when ready" }),
    ).toHaveFocus();
    fireEvent.keyDown(menu, { key: "Escape" });
    expect(
      screen.queryByRole("menu", { name: "Queued message options" }),
    ).toBeNull();
    expect(trigger).toHaveFocus();
  });
});

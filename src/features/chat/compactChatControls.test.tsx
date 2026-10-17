import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createRef, useRef, useState } from "react";
import { ComposerModelControls } from "./ComposerModelControls";
import { ComposerQueuedPrompts } from "./ComposerQueuedPrompts";
import { ChatHarnessControls } from "./ChatHarnessControls";
import { useSessionAccessModeStore } from "../../app/sessionAccessModeStore";
import { useWorkbenchStore } from "../../app/store";
import type { QueuedChatPrompt } from "../../domain/chatOwnership";

const initialAccess = useSessionAccessModeStore.getState();
const initialWorkbench = useWorkbenchStore.getState();
beforeEach(() =>
  useSessionAccessModeStore.setState({
    ready: true,
    modes: { fixture: "readOnly" },
  }),
);
afterEach(() => {
  cleanup();
  useSessionAccessModeStore.setState(initialAccess, true);
  useWorkbenchStore.setState(initialWorkbench, true);
});

describe("Compact chat controls", () => {
  it("opens access options from a shield, returns Escape to that opener, and toggles closed on a second pointer click", () => {
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "access-fixture",
          name: "Native fixture",
          description: "",
          installState: "available",
          source: "development",
          transport: "acp",
          bindingId: "fixture",
          accountModelDiscovery: false,
          accessModes: [
            {
              id: "readOnly",
              label: "Read-only",
              description: "Read workspace files; no writes or network.",
            },
          ],
        },
      ],
    });
    render(
      <ChatHarnessControls
        availableBinding
        canPrompt
        discoveryPhase="ready"
        modelLabelScope="compact"
        sessionHarnessId={null}
        sessionModelId={null}
        selectedHarnessId="access-fixture"
        selectedModelId={null}
        projectName={null}
        turnBusy={false}
        sessionConfig={null}
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
    expect(trigger).toHaveAccessibleDescription(
      "Read workspace files; no writes or network. Applies from your next message with this harness.",
    );
    expect(trigger.querySelector('svg[aria-hidden="true"]')).not.toBeNull();
    fireEvent.click(trigger);
    const dialog = screen.getByRole("menu", {
      name: "Access mode",
    });
    expect(dialog.parentElement).toBe(document.body);
    expect(
      screen.getByRole("menuitemradio", { name: "Read-only" }),
    ).toHaveFocus();
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(trigger).toHaveFocus();
    fireEvent.click(trigger);
    fireEvent.pointerDown(trigger);
    fireEvent.click(trigger);
    expect(screen.queryByRole("menu", { name: "Access mode" })).toBeNull();
  });

  it("gives the model trigger a meaningful icon and the confirmed identity as its description", () => {
    render(
      <ComposerModelControls
        triggerRef={createRef()}
        panelId="models"
        open={false}
        effortOpen={false}
        onToggle={() => {}}
        onToggleEffort={() => {}}
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
    expect(
      trigger.querySelector(".composer__model-icon.lucide-sparkles"),
    ).toHaveAttribute("aria-hidden", "true");
    expect(trigger.querySelector(".lucide-settings")).toBeNull();
  });

  it.each(["Escape", "Tab", "Shift+Tab"])(
    "opens queue actions outside the scrolling queue and returns focus on %s",
    (key) => {
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
      expect(
        fireEvent.keyDown(menu, {
          key: key === "Escape" ? key : "Tab",
          shiftKey: key === "Shift+Tab",
        }),
      ).toBe(false);
      expect(
        screen.queryByRole("menu", { name: "Queued message options" }),
      ).toBeNull();
      expect(trigger).toHaveFocus();
    },
  );

  it.each([
    ["First", ["First", "Middle", "Last"], "Middle", "menu"],
    ["Middle", ["First", "Middle", "Last"], "Last", "menu"],
    ["Last", ["First", "Middle", "Last"], "Middle", "menu"],
    ["Only", ["Only"], null, "menu"],
    ["Only", ["Only"], null, "inline"],
  ] as const)(
    "moves focus after deleting %s from %j to %s via %s",
    (selected, names, target, action) => {
      function QueueHarness() {
        const composerRef = useRef<HTMLTextAreaElement>(null);
        const [prompts, setPrompts] = useState<QueuedChatPrompt[]>(
          names.map((text) => ({
            id: text,
            text,
            attachedContext: [],
            bindingId: "binding",
            modelId: null,
            status: "queued",
          })),
        );
        return (
          <form>
            <ComposerQueuedPrompts
              composerRef={composerRef}
              prompts={prompts}
              disabled={false}
              onDelete={(id) =>
                setPrompts((current) =>
                  current.filter((prompt) => prompt.id !== id),
                )
              }
            />
            <textarea ref={composerRef} aria-label="Message agent" />
          </form>
        );
      }
      render(<QueueHarness />);
      const trigger = screen.getByRole("button", {
        name: `More options for queued message: ${selected}`,
      });
      if (action === "menu") {
        fireEvent.click(trigger);
        fireEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
      } else {
        fireEvent.click(
          screen.getByRole("button", {
            name: `Delete queued message: ${selected}`,
          }),
        );
      }
      expect(trigger).not.toBeInTheDocument();
      expect(
        screen.queryByRole("menu", { name: "Queued message options" }),
      ).toBeNull();
      expect(
        target
          ? screen.getByRole("button", {
              name: `More options for queued message: ${target}`,
            })
          : screen.getByRole("textbox", { name: "Message agent" }),
      ).toHaveFocus();
    },
  );

  it.each([
    ["First", ["First", "Last"], "Last"],
    ["Last", ["First", "Last"], "First"],
    ["Only", ["Only"], null],
  ] as const)(
    "preserves focus after deleting %s through pending acknowledgement and removal",
    async (selected, names, target) => {
      let acknowledgeDelete!: () => void;
      const acknowledgement = new Promise<void>((resolve) => {
        acknowledgeDelete = resolve;
      });
      function QueueHarness() {
        const composerRef = useRef<HTMLTextAreaElement>(null);
        const [prompts, setPrompts] = useState<QueuedChatPrompt[]>(
          names.map((text) => ({
            id: text,
            text,
            attachedContext: [],
            bindingId: "binding",
            modelId: null,
            status: "queued",
          })),
        );
        return (
          <form>
            <ComposerQueuedPrompts
              composerRef={composerRef}
              prompts={prompts}
              disabled={false}
              onDelete={async (id) => {
                setPrompts((current) =>
                  current.map((prompt) =>
                    prompt.id === id
                      ? { ...prompt, status: "deleting" }
                      : prompt,
                  ),
                );
                await acknowledgement;
                setPrompts((current) =>
                  current.filter((prompt) => prompt.id !== id),
                );
              }}
            />
            <textarea ref={composerRef} aria-label="Message agent" />
          </form>
        );
      }
      render(<QueueHarness />);
      const trigger = screen.getByRole("button", {
        name: `More options for queued message: ${selected}`,
      });
      fireEvent.click(trigger);
      fireEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
      const destination = target
        ? screen.getByRole("button", {
            name: `More options for queued message: ${target}`,
          })
        : screen.getByRole("textbox", { name: "Message agent" });
      expect(trigger).toBeInTheDocument();
      expect(trigger).toBeDisabled();
      expect(destination).toHaveFocus();
      await act(async () => {
        acknowledgeDelete();
      });
      expect(trigger).not.toBeInTheDocument();
      expect(
        screen.queryByRole("menu", { name: "Queued message options" }),
      ).toBeNull();
      expect(destination).toHaveFocus();
    },
  );

  it("does not reopen queue actions or steal focus after disabling and reenabling", () => {
    const queue = (disabled: boolean) => (
      <ComposerQueuedPrompts
        prompts={[
          {
            id: "queued",
            text: "Prompt",
            attachedContext: [],
            bindingId: "binding",
            modelId: null,
            status: "blocked",
          },
        ]}
        disabled={disabled}
      />
    );
    const { rerender } = render(queue(false));
    const trigger = screen.getByRole("button", {
      name: "More options for queued message: Prompt",
    });
    fireEvent.click(trigger);
    expect(
      screen.getByRole("menu", { name: "Queued message options" }),
    ).toBeVisible();
    rerender(queue(true));
    expect(
      screen.queryByRole("menu", { name: "Queued message options" }),
    ).toBeNull();
    screen
      .getByRole("button", { name: "Delete queued message: Prompt" })
      .focus();
    rerender(queue(false));
    expect(
      screen.queryByRole("menu", { name: "Queued message options" }),
    ).toBeNull();
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    expect(
      screen.getByRole("button", { name: "Delete queued message: Prompt" }),
    ).toHaveFocus();
  });
});

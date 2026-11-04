import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import {
  lazy,
  Suspense,
  useState,
  type ComponentType,
  type ReactNode,
} from "react";
import { AgentArtifactTabStrip } from "./AgentArtifactTabStrip";
import type { AgentArtifactTarget } from "../app/agentArtifactFollowStore";

afterEach(cleanup);

function Tabs({ changesTab }: { changesTab?: ReactNode }) {
  const [selected, select] = useState<AgentArtifactTarget>({
    kind: "tab",
    tabId: "b",
  });
  return (
    <AgentArtifactTabStrip
      changesTab={changesTab}
      artifacts={["a", "b", "c"].map((id, position) => ({
        id,
        position,
        title: id,
        kind: "browser",
        pinned: false,
        dirty: false,
      }))}
      selectedTarget={selected}
      focusedOwner={null}
      focusedPage={null}
      previewSelected={false}
      previewOpen={false}
      subagentOpen={false}
      subagentSelected={false}
      onSelectSubagent={() => {}}
      record={null}
      availableTargets={[]}
      activateTab={(tabId) => select({ kind: "tab", tabId })}
      closeTab={() => {}}
    />
  );
}
it("roves and selects resource tabs with arrows, Home and End", () => {
  render(<Tabs />);
  const tabs = screen.getAllByRole("tab");
  expect(tabs.map((tab) => tab.tabIndex)).toEqual([-1, 0, -1]);
  tabs[1].focus();
  fireEvent.keyDown(tabs[1], { key: "ArrowRight" });
  expect(tabs[2]).toHaveFocus();
  expect(tabs[2]).toHaveAttribute("aria-selected", "true");
  fireEvent.keyDown(tabs[2], { key: "ArrowRight" });
  expect(tabs[0]).toHaveFocus();
  fireEvent.keyDown(tabs[0], { key: "End" });
  expect(tabs[2]).toHaveFocus();
  fireEvent.keyDown(tabs[2], { key: "Home" });
  expect(tabs[0]).toHaveFocus();
  fireEvent.keyDown(tabs[0], { key: "ArrowLeft" });
  expect(tabs[2]).toHaveFocus();
});

it("keeps one tab stop when Changes finishes loading independently", async () => {
  let resolve!: (module: { default: ComponentType }) => void;
  const Changes = lazy(
    () =>
      new Promise<{ default: ComponentType }>((ready) => {
        resolve = ready;
      }),
  );
  render(
    <Tabs
      changesTab={
        <Suspense fallback={null}>
          <Changes />
        </Suspense>
      }
    />,
  );
  await act(async () =>
    resolve({
      default: () => (
        <button role="tab" aria-selected={false}>
          Changes
        </button>
      ),
    }),
  );
  expect(
    screen
      .getAllByRole("tab")
      .map((tab) => [tab.getAttribute("aria-selected"), tab.tabIndex]),
  ).toEqual([
    ["false", -1],
    ["false", -1],
    ["true", 0],
    ["false", -1],
  ]);
});

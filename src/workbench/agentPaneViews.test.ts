import { afterEach, expect, it } from "vitest";
import { useSubagentPaneStore } from "../app/subagentPaneStore";
import { useAgentChangesState } from "./agentChangesState";
import {
  closeAgentPaneView,
  MAX_PANE_VIEWS_PER_CHAT,
  openAgentPaneView,
  useAgentPaneViews,
} from "./agentPaneViews";

afterEach(() => {
  useAgentPaneViews.setState({ views: [], selected: null, opened: 0 });
  useAgentChangesState.getState().close();
  useSubagentPaneStore.setState({ open: null, selected: false, list: false });
});

const diagram = (chatId: string, title: string) => ({
  chatId,
  kind: "diagram" as const,
  title,
  source: `graph TD\n${title}`,
});

it("keeps at most eight views per Chat, dropping that Chat's oldest", () => {
  openAgentPaneView(diagram("chat-b", "Other"), "other");
  for (let index = 0; index <= MAX_PANE_VIEWS_PER_CHAT; index += 1)
    openAgentPaneView(diagram("chat-a", `D${index}`), `d${index}`);
  const views = useAgentPaneViews.getState().views;
  const own = views.filter((view) => view.chatId === "chat-a");
  expect(own.map((view) => view.title)).toEqual(
    Array.from({ length: 8 }, (_, index) => `D${index + 1}`),
  );
  expect(views.some((view) => view.chatId === "chat-b")).toBe(true);
});

it("reopens the same diagram as one tab and selects it", () => {
  openAgentPaneView(diagram("chat-a", "First"), "same");
  openAgentPaneView(diagram("chat-a", "Second"), "other");
  openAgentPaneView(diagram("chat-a", "First"), "same");
  const { views, selected } = useAgentPaneViews.getState();
  expect(views.map((view) => view.title)).toEqual(["Second", "First"]);
  expect(selected).toBe(views[1].id);
});

it("takes the canvas from Changes and Subagents and gives it back on close", () => {
  useAgentChangesState.setState({ selected: true });
  useSubagentPaneStore.setState({ selected: true });
  openAgentPaneView(diagram("chat-a", "Flow"), "flow");
  expect(useAgentChangesState.getState().selected).toBe(false);
  expect(useSubagentPaneStore.getState().selected).toBe(false);
  const [view] = useAgentPaneViews.getState().views;
  closeAgentPaneView(view.id);
  expect(useAgentPaneViews.getState()).toMatchObject({
    views: [],
    selected: null,
  });
});

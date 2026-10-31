import { expect, test } from "@playwright/test";
import {
  openScenario,
  expectNoViewportOverflow,
} from "./shared/visualBrowserPrimitives";

test("Workspace and Branch preferences reveal available metadata without displacing actions", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1380, height: 880 });
  await openScenario(page, "agent-single", "agent");
  await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const prefs = await import("/src/workbench/agentRailViewPrefs.ts");
    const state = useWorkbenchStore.getState();
    useWorkbenchStore.setState({
      project: {
        ...state.project!,
        name: "Workspace with a deliberately long project name",
      },
      projects: state.projects.map((project) =>
        project.id === state.project?.id
          ? {
              ...project,
              name: "Workspace with a deliberately long project name",
            }
          : project,
      ),
      repositories: {
        ...state.repositories,
        roots: state.repositories.roots.map((root) => ({
          ...root,
          branch: "feature/a-deliberately-long-branch-name",
        })),
      },
      selectedRepositoryStatus: {
        ...state.selectedRepositoryStatus!,
        branch: "feature/a-deliberately-long-branch-name",
      },
    });
    prefs.setAgentRailGrouping("status");
    prefs.setAgentRailShowField("workspace", true);
    prefs.setAgentRailShowField("branch", true);
  });
  const row = page.locator(".agent-rail-flat-list .project-chat-row").first();
  await expect(row.locator(".project-chat__workspace")).toBeVisible();
  await expect(row.locator(".project-chat__git")).toBeVisible();
  await row.hover();
  await expect(row.locator(".project-chat__actions")).toBeVisible();
  expect(
    await row.evaluate((node) => node.scrollWidth <= node.clientWidth),
  ).toBe(true);
  await page.evaluate(async () => {
    const prefs = await import("/src/workbench/agentRailViewPrefs.ts");
    prefs.setAgentRailShowField("workspace", false);
    prefs.setAgentRailShowField("branch", false);
  });
  await expect(row.locator(".project-chat__context")).toHaveCount(0);
  await page.evaluate(async () => {
    const prefs = await import("/src/workbench/agentRailViewPrefs.ts");
    prefs.setAgentRailGrouping("folder");
  });
  await expect(page.locator(".project-row__branch")).toHaveCount(0);
  await page.evaluate(async () => {
    const prefs = await import("/src/workbench/agentRailViewPrefs.ts");
    prefs.setAgentRailShowField("branch", true);
  });
  await expect(page.locator(".project-row__branch").first()).toBeVisible();
  await expectNoViewportOverflow(page);
});

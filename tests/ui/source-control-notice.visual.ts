import { expect, test } from "@playwright/test";

import { installLayoutMode } from "./shared/visualBrowserPrimitives";

test("repository switches preserve a failed stage without a new alert", async ({
  page,
}) => {
  await installLayoutMode(page, "editor");
  await page.goto("/");
  await expect(page.locator("main.workbench")).toBeVisible();
  await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const { browserSnapshot, browserRepository } =
      await import("/tests/ui/simulator/browserWorkspaceFixture.ts");
    const { desktop } = await import("/src/services/desktop.ts");
    const first = browserRepository.roots[0];
    useWorkbenchStore.setState({
      project: {
        ...browserSnapshot.project!,
        roots: [
          browserSnapshot.project!.roots[0],
          {
            id: "second-root",
            name: "Second",
            ordinal: 1,
            availability: "available",
            message: null,
          },
        ],
      },
      repositories: {
        ...browserRepository,
        roots: [first, { ...first, rootId: "second-root", changes: [] }],
      },
      repositoryRefs: [
        { rootId: first.rootId, relativePath: "" },
        { rootId: "second-root", relativePath: "" },
      ],
      selectedRepository: { rootId: first.rootId, relativePath: "" },
      selectedRepositoryStatus: first,
      repositoryInventoryPhase: "ready",
      repositoryStatusPhase: "ready",
      loadRepositoryInventory: async () => undefined,
    });
    // The simulator has no real Git locks; reject at its native command boundary.
    desktop.mutateGitFile = async () => {
      throw new Error("Git index is locked");
    };
  });
  await page
    .getByRole("button", { name: "Source Control", exact: true })
    .click();
  const source = page.getByLabel("Source Control project tools", {
    exact: true,
  });
  await source
    .getByRole("button", { name: /^Stage src\/workbench\/Workbench.tsx in/ })
    .click();
  const failure = source.getByRole("alert");
  await expect(failure).toContainText(
    "Source Control could not stage src/workbench/Workbench.tsx.",
  );
  await expect(failure).toHaveAttribute("aria-live", "assertive");
  await failure.getByRole("button", { name: "Details" }).click();
  const original = await failure.elementHandle();
  for (const name of [/^Second/, /^Sage/]) {
    await source.getByRole("button", { name: "Select repository" }).click();
    await page.getByRole("option", { name }).click();
    await expect(failure).toHaveCount(1);
    expect(
      await failure.evaluate((node, previous) => node === previous, original),
    ).toBe(true);
    await expect(failure).toContainText("Git index is locked");
    await expect(
      failure.getByRole("button", { name: "Details" }),
    ).toHaveAttribute("aria-expanded", "true");
  }
});

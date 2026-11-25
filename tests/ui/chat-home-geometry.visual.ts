import { expect, test } from "@playwright/test";
import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

test("unavailable chat recovery silently selects the first usable harness in catalog order", async ({
  page,
}) => {
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  await page.evaluate(async () => {
    const { installChatChromeFixture } =
      await import("/tests/ui/fixtures/chatChromeFixture.ts");
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const { desktop } = await import("/src/services/desktop.ts");
    installChatChromeFixture("codex", false);
    const harness = useWorkbenchStore.getState().harnesses[0];
    desktop.listHarnessModels = async () => [
      { id: "usable-model", name: "Usable model", isDefault: true },
    ];
    useWorkbenchStore.setState({
      harnesses: [
        { ...harness, installState: "unavailable" },
        {
          ...harness,
          id: "first-available",
          bindingId: "first-binding",
          name: "First available",
        },
        {
          ...harness,
          id: "second-available",
          bindingId: "second-binding",
          name: "Second available",
        },
      ],
    });
  });
  // The disabled composer carries the one explanation; recovery lives in the
  // harness picker beside it.
  const issue = page.getByText("Codex is unavailable.", { exact: true });
  await expect(issue).toHaveCount(1);
  await expect(issue).toBeVisible();
  await expect(
    page.getByRole("combobox", { name: "Message agent" }),
  ).toBeDisabled();
  const duplicates = page.getByText(
    /can't start this harness|reviewed local binding|selected harness is unavailable/,
  );
  await expect(duplicates).toHaveCount(0);
  await page.getByRole("button", { name: "Harness", exact: true }).click();
  await page
    .getByRole("dialog", { name: "Harness", exact: true })
    .getByRole("button", { name: "Continue in a new chat" })
    .click();
  const draft = page.getByRole("region", {
    name: "New chat draft",
    exact: true,
  });
  await expect(
    draft.getByRole("button", { name: "Harness", exact: true }),
  ).toHaveText("First available");
  await expect(
    draft.getByRole("button", { name: "Model", exact: true }),
  ).toHaveText("Usable model");
  await expect(
    draft.getByRole("combobox", { name: "Message agent" }),
  ).toHaveAttribute("placeholder", "Do anything");
  await expect(issue).toHaveCount(0);
  await expect(duplicates).toHaveCount(0);
  await expect(
    draft.getByRole("region", { name: "Composer notices" }),
  ).toHaveCount(0);
  await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const state = useWorkbenchStore.getState();
    state.discardNewChatDraft();
    useWorkbenchStore.setState({
      harnesses: state.harnesses.map((harness) => ({
        ...harness,
        installState: "unavailable",
      })),
    });
    await state.openNewChatDraft();
  });
  // With no usable harness the draft keeps one explanation on its disabled
  // composer; the harness picker carries the recovery.
  await expect(
    draft.getByText("Choose an available harness before sending.", {
      exact: true,
    }),
  ).toHaveCount(1);
  await expect(duplicates).toHaveCount(0);
  await expect(
    draft.getByRole("combobox", { name: "Message agent" }),
  ).toBeDisabled();
});

for (const width of [1400, 900]) {
  test(`new chat project subtitle is centred under its heading at ${width}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 1000 });
    await openScenario(page, "agent-single", "agent");
    await settleChatProjections(page);
    await page.evaluate(async () => {
      const { installChatChromeFixture } =
        await import("/tests/ui/fixtures/chatChromeFixture.ts");
      const { useWorkbenchStore } = await import("/src/app/store.ts");
      installChatChromeFixture("codex", true);
      const project = useWorkbenchStore.getState().project!;
      useWorkbenchStore.setState({ project: { ...project, name: "Local" } });
    });
    const home = page.getByRole("region", { name: "New chat", exact: true });
    const heading = home.getByRole("heading", {
      name: "What should we work on?",
    });
    const subtitle = home.getByRole("button", { name: "Project: Local" });
    await expect(heading).toBeVisible();
    await expect(subtitle).toBeVisible();
    await page.evaluate(() => document.fonts.ready);
    const headingBox = (await heading.boundingBox())!;
    const textBox = await subtitle
      .locator(".searchable-picker__trigger-label")
      .evaluate((element) => {
        const range = document.createRange();
        range.selectNodeContents(element);
        const box = range.getBoundingClientRect();
        return { x: box.x, width: box.width };
      });
    expect(
      Math.abs(
        textBox.x + textBox.width / 2 - (headingBox.x + headingBox.width / 2),
      ),
    ).toBeLessThanOrEqual(1);
  });
}

import { expect, type Page } from "@playwright/test";

/** Advertise a second choice when a scenario exercises the model picker. */
export async function advertiseAlternativeModel(page: Page) {
  await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    useWorkbenchStore.setState((state) => ({
      models: [
        ...state.models,
        { id: "alternative", name: "Alternative model" },
      ],
    }));
  });
  await expect(
    page
      .locator("form.composer")
      .first()
      .getByRole("button", { name: "Model", exact: true }),
  ).toBeVisible();
}

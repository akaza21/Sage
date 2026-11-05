import { expect, test } from "@playwright/test";
import { openScenario } from "./shared/visualBrowserPrimitives";

for (const width of [480, 1280]) {
  test(`live file markdown preview shares bounded blocks and unmounts at ${width}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 800 });
    await openScenario(page, "file", "editor");
    if (
      await page
        .getByRole("dialog", { name: "Explorer project tools" })
        .isVisible()
    )
      await page.keyboard.press("Escape");
    await page.evaluate(async () => {
      const [{ useWorkbenchStore }, { browserEditor }] = await Promise.all([
        import("/src/app/store.ts"),
        import("/src/services/browserEditor.ts"),
      ]);
      const state = useWorkbenchStore.getState();
      const tab = state.tabs.find((tab) => tab.id === state.activeTabId);
      if (
        tab?.kind !== "file" ||
        !tab.resource ||
        state.activeEditor?.phase !== "ready"
      )
        throw new Error("Expected live file fixture");
      const resource = { ...tab.resource, relativePath: "docs/preview.md" };
      const content =
        "# Preview\n\n- [ ] pending\n- [x] complete\n\n| Name | Value |\n| :--- | ---: |\n| safe | content |\n\n```js\nconst safe = '<script>';\n```\n\n<script>literal</script>";
      const opened = browserEditor.discard(resource);
      browserEditor.snapshot({
        resource,
        base: opened.base,
        lineEnding: "lf",
        revision: opened.buffer.revision + 1,
        content,
      });
      const document = browserEditor.save(resource).document;
      useWorkbenchStore.setState({
        tabs: state.tabs.map((candidate) =>
          candidate.id === tab.id
            ? { ...tab, resource, title: "preview.md" }
            : candidate,
        ),
        activeEditor: { ...state.activeEditor, resource, document, content },
      });
    });
    await page
      .getByRole("button", { name: "Preview file", exact: true })
      .click();
    const preview = page.getByRole("article", { name: "Markdown preview" });
    await expect(
      preview.getByRole("heading", { name: "Preview", exact: true }),
    ).toBeVisible();
    await expect(
      preview.getByRole("checkbox", { name: "pending" }),
    ).not.toBeChecked();
    await expect(
      preview.getByRole("checkbox", { name: "complete" }),
    ).toBeChecked();
    await expect(
      preview.getByRole("checkbox", { name: "complete" }),
    ).toBeDisabled();
    await expect(
      preview.getByRole("cell", { name: "content", exact: true }),
    ).toBeVisible();
    await expect(preview.locator("pre")).toHaveText("const safe = '<script>';");
    await expect(preview.locator("script")).toHaveCount(0);
    await expect(page.locator(".cm-editor")).toHaveCount(0);
    await page
      .getByRole("button", { name: "Return to editor", exact: true })
      .click();
    await expect(preview).toHaveCount(0);
    await expect(
      page.getByRole("textbox", { name: "Edit docs/preview.md" }),
    ).toBeVisible();
  });
}

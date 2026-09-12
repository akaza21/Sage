import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { AcpItem } from "../../../domain/acp";

import { ImageItem } from "./ImageItem";

afterEach(cleanup);

const png =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=";

function item(dataUrl = `data:image/png;base64,${png}`, id = "image-one") {
  return {
    id,
    kind: "image",
    payload: { mediaType: "image/png", dataUrl },
  } as AcpItem as Extract<AcpItem, { kind: "image" }>;
}

describe("ImageItem", () => {
  it("opens only the admitted data URL in an in-Sage dialog and restores focus", async () => {
    render(<ImageItem item={item()} />);
    const trigger = screen.getByRole("button", { name: "Open image viewer" });
    fireEvent.click(trigger);

    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveClass("acp-image__dialog");
    expect(
      screen.getAllByRole("img", { name: "Image provided by the agent" }),
    ).toHaveLength(2);
    expect(
      screen.getByRole("button", { name: "Close image viewer" }),
    ).toBeVisible();

    fireEvent.keyDown(dialog, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(() => expect(trigger).toHaveFocus());
  });

  it("never reopens A after direct A-open→B→A identity changes", async () => {
    const imageA = `data:image/png;base64,${png}`;
    const imageB = `data:image/png;base64,${png.replace(/.$/, "A")}`;
    const { rerender } = render(<ImageItem item={item(imageA, "image-a")} />);
    fireEvent.click(screen.getByRole("button", { name: "Open image viewer" }));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    rerender(<ImageItem item={item(imageB, "image-b")} />);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    rerender(<ImageItem item={item(imageA, "image-a")} />);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("never reopens A after direct A-open→invalid→A identity changes", async () => {
    const imageA = `data:image/png;base64,${png}`;
    const { rerender } = render(<ImageItem item={item(imageA, "image-a")} />);
    fireEvent.click(screen.getByRole("button", { name: "Open image viewer" }));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    rerender(
      <ImageItem item={item("https://example.invalid/image.png", "image-b")} />,
    );
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    rerender(<ImageItem item={item(imageA, "image-a")} />);
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.mock("../../services/imageArtifactDesktop", () => ({
  getChatImageThumbnail: vi.fn(),
}));

import { getChatImageThumbnail } from "../../services/imageArtifactDesktop";
import { ImageAttachmentThumbnail } from "./ImageAttachmentThumbnail";

const fetchThumbnail = vi.mocked(getChatImageThumbnail);
const owner = { projectId: "project", projectGeneration: "1", chatId: "chat" };

beforeEach(() => {
  fetchThumbnail.mockReset();
});
afterEach(cleanup);

it("shows a bounded native thumbnail for its exact Chat owner", async () => {
  fetchThumbnail.mockResolvedValue("data:image/png;base64,YQ==");
  render(
    <ImageAttachmentThumbnail
      owner={owner}
      artifactId="artifact"
      label="Screenshot"
    />,
  );
  expect(await screen.findByAltText("Screenshot thumbnail")).toHaveAttribute(
    "src",
    "data:image/png;base64,YQ==",
  );
  expect(fetchThumbnail).toHaveBeenCalledWith({
    ...owner,
    artifactId: "artifact",
  });
});

it("honestly labels an evicted thumbnail", async () => {
  fetchThumbnail.mockResolvedValue(null);
  render(
    <ImageAttachmentThumbnail
      owner={owner}
      artifactId="artifact"
      label="Screenshot"
    />,
  );
  expect(await screen.findByText("Image no longer stored")).toBeInTheDocument();
});

it("never displays a previous Chat thumbnail after the owner changes", async () => {
  fetchThumbnail.mockImplementation((request) =>
    request?.chatId === "chat"
      ? Promise.resolve("data:image/png;base64,YQ==")
      : new Promise<string | null>(() => undefined),
  );
  const view = render(
    <ImageAttachmentThumbnail
      owner={owner}
      artifactId="artifact"
      label="Screenshot"
    />,
  );
  await screen.findByAltText("Screenshot thumbnail");
  view.rerender(
    <ImageAttachmentThumbnail
      owner={{ ...owner, chatId: "other-chat" }}
      artifactId="artifact"
      label="Screenshot"
    />,
  );
  expect(screen.queryByAltText("Screenshot thumbnail")).toBeNull();
  expect(screen.getByText("Loading image…")).toBeInTheDocument();
});

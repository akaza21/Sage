import { render, screen, cleanup } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { ComposerNoticeFrame } from "./ComposerStatusNotices";
import { composerProblem } from "./ComposerStatusNotices";

afterEach(cleanup);

it("renders three channels for the same attachment problem once at its owner", () => {
  render(
    <ComposerNoticeFrame
      workbenchError={composerProblem(
        "upload",
        "chat-a/image-a",
        "Upload failed.",
      )}
      chatError={composerProblem(
        "upload",
        "chat-a/image-a",
        "Cannot send the image.",
      )}
      imageStatus={composerProblem(
        "upload",
        "chat-a/image-a",
        "The attachment upload failed.",
        "attachment",
        "info",
      )}
    />,
  );
  expect(screen.getAllByRole("alert")).toHaveLength(1);
  expect(screen.getByRole("alert")).toHaveTextContent(
    "The attachment upload failed.",
  );
  expect(screen.queryByText("Cannot send the image.")).toBeNull();
});
it("keeps identical messages about two attachments separate", () => {
  render(
    <ComposerNoticeFrame
      workbenchError={composerProblem(
        "upload",
        "image-a",
        "Upload failed.",
        "attachment",
      )}
      chatError={composerProblem(
        "upload",
        "image-b",
        "Upload failed.",
        "attachment",
      )}
      imageStatus={null}
    />,
  );
  expect(screen.getAllByRole("alert")).toHaveLength(2);
});

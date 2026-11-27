import { expect, it, vi } from "vitest";

import { attachPickedImages } from "./pickedImages";

it("hands a draft's picked image bytes to the draft's image intake as a file", async () => {
  const intake = vi.fn();
  const count = await attachPickedImages(
    null,
    {
      files: null,
      draftImages: [
        { mediaType: "image/png", label: "shot.png", dataBase64: "iVBORw==" },
      ],
    },
    intake,
  );
  expect(count).toBe(1);
  const [[[file]]] = intake.mock.calls as [[[File]]];
  expect([file.name, file.type]).toEqual(["shot.png", "image/png"]);
  expect([...new Uint8Array(await file.arrayBuffer())]).toEqual([
    0x89, 0x50, 0x4e, 0x47,
  ]);
});

import { expect, it } from "vitest";

import { browserEditor } from "../../../src/services/browserEditor";
import {
  editorContent,
  editorDocument,
  visualFileGitBase,
} from "./visualDocumentFixtures";

it("keeps visual File edits recoverable and saveable without staging them", () => {
  const resource = { rootId: "visual-root", relativePath: "src/recovery.ts" };
  const staged = browserEditor.stagedText(resource, null);
  const document = editorDocument(resource);
  expect(browserEditor.open(resource).buffer.content).toBe(editorContent);

  const content = "const total = count(items);\n";
  expect(
    browserEditor.snapshot({
      resource,
      base: document.base,
      lineEnding: "lf",
      revision: 1,
      content,
    }),
  ).toMatchObject({ revision: 1, dirty: true });
  expect(browserEditor.open(resource)).toMatchObject({
    buffer: { content },
    dirty: true,
    conflict: null,
  });
  expect(browserEditor.save(resource)).toMatchObject({
    outcome: "saved",
    document: { buffer: { content }, dirty: false, conflict: null },
  });
  expect(browserEditor.open(resource).buffer.content).toBe(content);
  expect(visualFileGitBase(resource, null)).toEqual(staged);
  if (staged.state !== "ready") throw new Error("Expected staged fixture");
  expect(visualFileGitBase(resource, staged.oid)).toEqual({
    state: "unchanged",
    oid: staged.oid,
  });

  expect(editorDocument(resource).buffer.content).toBe(editorContent);
  expect(browserEditor.open(resource)).toMatchObject({
    buffer: { content: editorContent },
    dirty: false,
    conflict: null,
  });
});

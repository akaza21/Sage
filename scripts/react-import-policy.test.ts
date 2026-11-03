// @vitest-environment node

import path from "node:path";
import { ESLint } from "eslint";
import { beforeAll, describe, expect, it } from "vitest";

const root = path.resolve(import.meta.dirname, "..");
const linter = new ESLint({
  cwd: root,
  overrideConfigFile: path.join(root, "eslint.config.js"),
});

// Load the real ESLint configuration once, outside the rule assertions' clock.
beforeAll(async () => {
  await linter.calculateConfigForFile(
    path.join(root, "src/react-import-policy-fixture.ts"),
  );
}, 30_000);

describe("React import policy", () => {
  it.each(["react", "react/jsx-runtime"])(
    "bans namespace imports of %s in JS and TS",
    async (source) => {
      for (const extension of ["js", "ts"]) {
        const [result] = await linter.lintText(
          `import * as Runtime from '${source}'; void Runtime;`,
          {
            filePath: path.join(
              root,
              `src/react-import-policy-fixture.${extension}`,
            ),
          },
        );
        expect(result.messages).toEqual(
          expect.arrayContaining([
            expect.objectContaining({
              ruleId: "no-restricted-syntax",
              severity: 2,
              message: expect.stringMatching(/React namespace imports/),
            }),
          ]),
        );
      }
    },
  );
  it("allows named/default React imports and unrelated namespaces", async () => {
    const [result] = await linter.lintText(
      "import React, {useTransition} from 'react'; import {jsx} from 'react/jsx-runtime'; import * as Path from 'node:path'; void [React,useTransition,jsx,Path];",
      { filePath: path.join(root, "src/react-import-policy-fixture.ts") },
    );
    expect(result.errorCount).toBe(0);
  });
});

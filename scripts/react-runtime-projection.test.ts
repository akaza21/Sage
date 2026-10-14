// @vitest-environment node

import path from "node:path";
import { build } from "vite";
import { afterEach, describe, expect, it, vi } from "vitest";

import { desktopReactRuntimeProjectionPlugin } from "./reactRuntimeProjection";

async function buildProjectionFixture(
  source: string,
  define?: Record<string, string>,
) {
  const root = path.resolve(import.meta.dirname, "..");
  const entry = path
    .join(root, "src/runtime-projection-fixture.ts")
    .replaceAll("\\", "/");
  return build({
    configFile: false,
    mode: "desktop-native",
    logLevel: "silent",
    define,
    plugins: [
      desktopReactRuntimeProjectionPlugin("build", "desktop-native"),
      {
        name: "runtime-projection-fixture",
        resolveId(id) {
          if (id === entry) return entry;
        },
        load(id) {
          if (id === entry) return source;
        },
      },
    ],
    build: {
      write: false,
      minify: true,
      rolldownOptions: { input: entry, preserveEntrySignatures: "strict" },
    },
  });
}

async function evaluateProjectionFixture(
  source: string,
  define?: Record<string, string>,
) {
  const result = await buildProjectionFixture(source, define);
  if (Array.isArray(result) || !("output" in result))
    throw new Error("Expected one build");
  const chunk = result.output.find((item) => item.type === "chunk");
  if (!chunk || chunk.type !== "chunk")
    throw new Error("Expected fixture output");
  return import(
    `data:text/javascript;base64,${Buffer.from(chunk.code).toString("base64")}`
  );
}

describe("desktop React runtime projection", () => {
  afterEach(() => vi.unstubAllEnvs());
  it("leaves browser and development resolution untouched", () => {
    expect(
      desktopReactRuntimeProjectionPlugin("serve", "desktop-native"),
    ).toBeUndefined();
    expect(
      desktopReactRuntimeProjectionPlugin("build", "development"),
    ).toBeUndefined();
  });

  it.each([
    ["production", undefined],
    ["development", undefined],
    ["production", '"development"'],
    ["development", '"production"'],
    ["development", " 'production' "],
    ["production", "'development'"],
  ] as const)(
    "projects every installed export for NODE_ENV=%s with define=%s",
    async (environment, definedEnvironment) => {
      vi.stubEnv("NODE_ENV", environment);
      const root = path
        .resolve(import.meta.dirname, "..")
        .replaceAll("\\", "/");
      const fixture = await evaluateProjectionFixture(
        `
      import * as React from 'react';
      import * as Jsx from 'react/jsx-runtime';
      import originalReact from '${root}/node_modules/react/index.js';
      import originalJsx from '${root}/node_modules/react/jsx-runtime.js';
      export const reactKeys = Object.keys(React).filter(key => key !== 'default').sort();
      export const jsxKeys = Object.keys(Jsx).filter(key => key !== 'default').sort();
      export const originalReactKeys = Object.keys(originalReact).sort();
      export const originalJsxKeys = Object.keys(originalJsx).sort();
      export const identities = Object.keys(originalReact).every(key => React[key] === originalReact[key]) && Object.keys(originalJsx).every(key => Jsx[key] === originalJsx[key]);
      export const transition = React.useTransition === originalReact.useTransition;
      export const transitionType = typeof React.useTransition;
    `,
        definedEnvironment === undefined
          ? undefined
          : { "process.env.NODE_ENV": definedEnvironment },
      );
      expect(fixture.reactKeys).toEqual(fixture.originalReactKeys);
      expect(fixture.jsxKeys).toEqual(fixture.originalJsxKeys);
      expect(fixture.identities).toBe(true);
      expect(fixture.transition).toBe(true);
      expect(fixture.transitionType).toBe("function");
    },
  );

  it("rejects development-only React exports for a single-quoted production define", async () => {
    await expect(
      buildProjectionFixture(
        "import {act} from 'react'; export const unavailable = act;",
        { "process.env.NODE_ENV": "'production'" },
      ),
    ).rejects.toThrow(/Desktop React runtime projection.*MISSING_EXPORT/s);
  });

  it.each(["process.env.CUSTOM_ENV", "'pro' + 'duction'", "`production`"])(
    "rejects an unverifiable NODE_ENV override %s",
    async (expression) => {
      await expect(
        buildProjectionFixture(
          "import {useTransition} from 'react'; export const hook = useTransition;",
          { "process.env.NODE_ENV": expression },
        ),
      ).rejects.toThrow(
        /Desktop React runtime projection cannot verify process.env.NODE_ENV/,
      );
    },
  );

  it.each(["react", "react/jsx-runtime"])(
    "fails closed on an undefined namespace member from %s",
    async (source) => {
      await expect(
        buildProjectionFixture(
          `import * as Runtime from '${source}'; export const missing = Runtime.bx3MissingExport;`,
        ),
      ).rejects.toThrow(
        /Desktop React runtime projection.*IMPORT_IS_UNDEFINED/s,
      );
    },
  );

  it.each(["react", "react/jsx-runtime"])(
    "fails clearly on a missing named export from %s",
    async (source) => {
      await expect(
        buildProjectionFixture(
          `import {bx3MissingExport} from '${source}'; export const missing = bx3MissingExport;`,
        ),
      ).rejects.toThrow(/Desktop React runtime projection.*MISSING_EXPORT/s);
    },
  );

  it.each(["react", "react/jsx-runtime"])(
    "fails clearly on a missing re-export from %s",
    async (source) => {
      await expect(
        buildProjectionFixture(`export {bx3MissingExport} from '${source}';`),
      ).rejects.toThrow(/Desktop React runtime projection.*MISSING_EXPORT/s);
    },
  );

  it("validates emitted runtime imports without rejecting erased React types", async () => {
    await expect(
      buildProjectionFixture(
        `import type {ComponentProps} from 'react'; import {type PropsWithChildren, useTransition} from 'react'; type Props = PropsWithChildren<ComponentProps<'div'>>; export const hook: unknown = useTransition;`,
      ),
    ).resolves.toBeDefined();
  });

  it.each(["production", "development"])(
    "preserves React and JSX identities for %s without CommonJS call indirection",
    async (environment) => {
      vi.stubEnv("NODE_ENV", environment);
      const root = path.resolve(import.meta.dirname, "..");
      const entry = path
        .join(root, "src/runtime-projection-fixture.ts")
        .replaceAll("\\", "/");
      const result = await build({
        configFile: false,
        mode: "desktop-native",
        plugins: [
          desktopReactRuntimeProjectionPlugin("build", "desktop-native"),
          {
            name: "runtime-projection-fixture",
            resolveId(id) {
              if (id === entry) return entry;
            },
            load(id) {
              if (id !== entry) return;
              return `import React, {Component,Fragment,StrictMode,Suspense,createContext,createElement,forwardRef,lazy,memo,useCallback,useContext,useEffect,useId,useImperativeHandle,useLayoutEffect,useMemo,useRef,useState,useSyncExternalStore} from 'react';
              import {jsx,jsxs,Fragment as JsxFragment} from 'react/jsx-runtime';
              import originalReact from '${root.replaceAll("\\", "/")}/node_modules/react/index.js';
              import originalJsx from '${root.replaceAll("\\", "/")}/node_modules/react/jsx-runtime.js';
              const projected = {Component,Fragment,StrictMode,Suspense,createContext,createElement,forwardRef,lazy,memo,useCallback,useContext,useEffect,useId,useImperativeHandle,useLayoutEffect,useMemo,useRef,useState,useSyncExternalStore};
              export const identities = Object.entries(projected).every(([key,value])=>value === originalReact[key]) && React === originalReact && jsx === originalJsx.jsx && jsxs === originalJsx.jsxs && JsxFragment === originalJsx.Fragment;
              export const element = jsxs('section',{children:[jsx('span',{children:'unchanged'}),jsx(Fragment,{children:'text'})]});`;
            },
          },
        ],
        build: {
          write: false,
          minify: true,
          rolldownOptions: { input: entry, preserveEntrySignatures: "strict" },
        },
      });
      if (Array.isArray(result) || !("output" in result))
        throw new Error("Expected a single build");
      const chunk = result.output.find((item) => item.type === "chunk");
      if (!chunk || chunk.type !== "chunk")
        throw new Error("Expected fixture output");
      const fixture = await import(
        `data:text/javascript;base64,${Buffer.from(chunk.code).toString("base64")}`
      );
      expect(fixture.identities).toBe(true);
      expect(fixture.element.type).toBe("section");
      expect(fixture.element.props.children[0].props.children).toBe(
        "unchanged",
      );
      expect(fixture.element.props.children[1].props.children).toBe("text");
    },
  );
});

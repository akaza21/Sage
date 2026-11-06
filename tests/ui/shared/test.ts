// Boundary: the visual lane's Playwright `test`, extended only to give every
// page an anchored dynamic import. Specs import `test` from here and keep
// importing types and `expect` from either module.
//
// Page code evaluated through Playwright awaits `import()` of Sage modules.
// V8's inspector holds an awaited evaluation promise strongly only until a
// context, session or object-group event makes that handle weak; a promise
// whose chain is then unreachable is collected and the call fails with
// "Resulting promise was garbage collected". A pending `import()` was the one
// unrooted link in every such failure, so `sageImport` keeps each import
// promise referenced from the page until it settles. Module identity and
// lazy-loading boundaries are unchanged: it is the same `import()`, issued
// when the test asks for the module.
import { test as base } from "@playwright/test";

export * from "@playwright/test";

declare global {
  // Typed like `import()` of an unresolved specifier in these page bodies.
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  function sageImport(specifier: string): Promise<any>;
}

function installAnchoredImport() {
  // Bounded by the imports pending at once; each entry leaves on settle.
  const pending = new Set<Promise<unknown>>();
  Object.defineProperty(globalThis, "sageImport", {
    value: (specifier: string) => {
      const loading = import(specifier);
      pending.add(loading);
      const release = () => pending.delete(loading);
      loading.then(release, release);
      return loading;
    },
  });
}

export const test = base.extend({
  context: async ({ context }, provide) => {
    await context.addInitScript(installAnchoredImport);
    await provide(context);
  },
});

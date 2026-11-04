import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import type { Plugin } from "vite";

const require = createRequire(import.meta.url);
const sourceRoot = path
  .resolve(path.dirname(fileURLToPath(import.meta.url)), "../src")
  .replaceAll("\\", "/");
const reactId = "\0sage-react-runtime";
const jsxId = "\0sage-jsx-runtime";

interface ProjectionDiagnostic {
  code?: string;
  id?: string;
  exporter?: string;
  message: string;
}

function isProjectionDiagnostic(log: ProjectionDiagnostic) {
  return (
    (log.code === "IMPORT_IS_UNDEFINED" || log.code === "MISSING_EXPORT") &&
    [reactId, jsxId].some(
      (id) =>
        log.id === id ||
        log.exporter === id ||
        log.message.includes(id) ||
        log.message.includes(id.replace("\0", "\\0")),
    )
  );
}

function projectionError(log: ProjectionDiagnostic) {
  return `Desktop React runtime projection rejected ${log.code}: ${log.message}`;
}

function runtimeBindings(runtime: Record<string, unknown>) {
  const bindings = Object.keys(runtime);
  if (
    bindings.some(
      (name) => !/^[A-Za-z_$][\w$]*$/.test(name) || name === "default",
    )
  ) {
    throw new Error(
      "Desktop React runtime projection found an unsupported runtime export name",
    );
  }
  return bindings;
}

function projectBindings(bindings: readonly string[]) {
  // React's runtime exports are static values. Unused projections need no
  // property read, so demand-loaded hooks do not retain every runtime export.
  return bindings
    .map(
      (name) =>
        `export const ${name} = /* @__PURE__ */ (() => runtime.${name})();`,
    )
    .join(" ");
}

function runtimeEnvironment(
  definedEnvironment: unknown,
  isProduction: boolean,
) {
  if (definedEnvironment === undefined)
    return isProduction ? "production" : "development";
  const literal =
    typeof definedEnvironment === "string"
      ? definedEnvironment.trim().match(/^(["'])([^\\\r\n"']*)\1$/)
      : null;
  if (!literal) {
    throw new Error(
      "Desktop React runtime projection cannot verify process.env.NODE_ENV; use a plain single- or double-quoted string literal.",
    );
  }
  return literal[2] === "production" ? "production" : "development";
}

export function desktopReactRuntimeProjectionPlugin(
  command: string,
  mode: string,
): Plugin | undefined {
  if (command !== "build" || mode !== "desktop-native") return undefined;
  const reactPath = require.resolve("react").replaceAll("\\", "/");
  const jsxPath = require.resolve("react/jsx-runtime").replaceAll("\\", "/");
  let reactBindings = runtimeBindings(require("react"));
  let jsxBindings = runtimeBindings(require("react/jsx-runtime"));
  let sameJsx = false;
  return {
    name: "sage-desktop-react-runtime-projection",
    enforce: "pre",
    configResolved(config) {
      const definedEnvironment = config.define?.["process.env.NODE_ENV"];
      const environment = runtimeEnvironment(
        definedEnvironment,
        config.isProduction,
      );
      const production = environment === "production";
      const directory = path.join(path.dirname(reactPath), "cjs");
      const reactRuntime = require(
        path.join(directory, `react.${environment}.js`),
      );
      const jsxRuntime = require(
        path.join(directory, `react-jsx-runtime.${environment}.js`),
      );
      reactBindings = runtimeBindings(reactRuntime);
      jsxBindings = runtimeBindings(jsxRuntime);
      if (
        !["jsx", "jsxs", "Fragment"].every((name) => jsxBindings.includes(name))
      ) {
        throw new Error(
          "Desktop React runtime projection found an incomplete JSX runtime export surface",
        );
      }
      sameJsx = production && jsxRuntime.jsx === jsxRuntime.jsxs;
    },
    onLog(_level, log) {
      if (isProjectionDiagnostic(log)) this.error(projectionError(log));
    },
    resolveId(source, importer) {
      if (!importer?.replaceAll("\\", "/").startsWith(`${sourceRoot}/`))
        return undefined;
      if (source === "react") return reactId;
      if (source === "react/jsx-runtime") return jsxId;
      return undefined;
    },
    transform: {
      order: "post",
      handler(code, id) {
        if (!id.replaceAll("\\", "/").startsWith(`${sourceRoot}/`)) return;
        for (const node of this.parse(code).body) {
          if (
            node.type !== "ImportDeclaration" &&
            node.type !== "ExportNamedDeclaration"
          )
            continue;
          const source = node.source?.value;
          if (source !== "react" && source !== "react/jsx-runtime") continue;
          const bindings =
            source === "react" ? ["default", ...reactBindings] : jsxBindings;
          for (const specifier of node.specifiers) {
            const imported =
              specifier.type === "ImportSpecifier"
                ? specifier.imported
                : specifier.type === "ExportSpecifier"
                  ? specifier.local
                  : null;
            const name =
              specifier.type === "ImportDefaultSpecifier"
                ? "default"
                : imported?.type === "Identifier"
                  ? imported.name
                  : imported?.value;
            if (
              name !== undefined &&
              name !== null &&
              !bindings.includes(String(name))
            ) {
              this.error(
                projectionError({
                  code: "MISSING_EXPORT",
                  message: `${source} does not export ${String(name)} requested by ${id}; use an installed runtime export.`,
                }),
              );
            }
          }
        }
      },
    },
    load(id) {
      if (id === reactId)
        return `import runtime from ${JSON.stringify(reactPath)}; export default runtime; ${projectBindings(reactBindings)}`;
      if (id === jsxId)
        return `import runtime from ${JSON.stringify(jsxPath)}; ${projectBindings(jsxBindings.filter((name) => !sameJsx || name !== "jsxs"))} ${sameJsx ? "export {jsx as jsxs};" : ""}`;
      return undefined;
    },
  };
}

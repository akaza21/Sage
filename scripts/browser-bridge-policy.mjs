import { existsSync, readFileSync, readdirSync } from "node:fs";
import path from "node:path";

export function browserBridgeBundleErrors(source) {
  return /SAGE_DEV_BROWSER_BRIDGE|sage-browser-bridge:|installBrowserBridge/u.test(
    source,
  )
    ? ["desktop artifact contains browser bridge code"]
    : [];
}

export function browserBridgeConfigurationErrors(file, source) {
  if (file === "package.json") {
    const scripts = JSON.parse(source).scripts ?? {};
    return Object.entries(scripts).flatMap(([name, command]) =>
      /browser-bridge|dev:browser/u.test(command) &&
      !(
        name === "dev:browser" &&
        command === "tauri dev --features browser-bridge"
      )
        ? [`${file}: browser-bridge is allowed only in dev:browser`]
        : [],
    );
  }
  if (file.endsWith("Cargo.toml")) {
    return /debug-assertions\s*=\s*true/u.test(source) ||
      /^\s*(?!browser-bridge\s*=)[\w-]+\s*=\s*\[[^\]]*"browser-bridge"/mu.test(
        source,
      )
      ? [`${file}: browser-bridge must remain debug-only and opt-in`]
      : [];
  }
  if (file.includes("/workflows/")) {
    return source
      .split(/\r?\n|&&|\|\||[;|]/u)
      .flatMap((line) =>
        line.includes("browser-bridge") &&
        !(
          /^\s*(?:-\s*)?(?:run:\s*)?["']?cargo\s+(?:test|clippy|check)\b/u.test(
            line,
          ) && !/--release|--profile|[$`<>]/u.test(line)
        )
          ? [
              `${file}: browser-bridge cannot enter workflow build configuration`,
            ]
          : [],
      );
  }
  return source.includes("browser-bridge")
    ? [`${file}: browser-bridge cannot enter bundle configuration`]
    : [];
}

export function browserBridgePolicyErrors(root) {
  const errors = [];
  const inspect = (relative) => {
    const file = path.join(root, relative);
    if (!existsSync(file)) return;
    errors.push(
      ...browserBridgeConfigurationErrors(relative, readFileSync(file, "utf8")),
    );
  };
  for (const file of ["package.json", "Cargo.toml", "src-tauri/Cargo.toml"])
    inspect(file);
  for (const directory of ["src-tauri", ".github/workflows"]) {
    if (!existsSync(path.join(root, directory))) continue;
    for (const entry of readdirSync(path.join(root, directory), {
      withFileTypes: true,
    })) {
      if (
        entry.isFile() &&
        (/^tauri.*\.(?:json|json5|toml)$/u.test(entry.name) ||
          /\.ya?ml$/u.test(entry.name))
      )
        inspect(`${directory}/${entry.name}`);
    }
  }
  const assets = path.join(root, "dist/assets");
  if (existsSync(assets))
    for (const file of readdirSync(assets)) {
      if (/\.js$/u.test(file))
        errors.push(
          ...browserBridgeBundleErrors(
            readFileSync(path.join(assets, file), "utf8"),
          ),
        );
    }
  return errors;
}

// Which checks a set of changed paths needs. CI's plan job and the local
// `gate changed` mode both read this, so a change runs the same lanes in
// both places.

// Paths the Rust crate compiles in or reads in its tests.
export const rustPathPattern =
  /^(Cargo\.(?:toml|lock)$|deny\.toml$|crates\/|src-tauri\/|testdata\/|tests\/fixtures\/|docs\/work-items\/|scripts\/ade|scripts\/(?:cargo-features|change-scope)\.mjs$|src\/features\/chat\/acp\/|rust-toolchain\.toml$|\.github\/workflows\/)/;

// Inputs every Vitest file depends on: a change here can affect any test, so
// the whole suite runs instead of the tests that import the changed files.
const vitestWidePattern =
  /^(package(-lock)?\.json$|vite\.config(\.[a-z]+)?\.ts$|tsconfig[^/]*\.json$|src\/test\/)/;

// Files Vitest may trace through imports to the tests that cover them;
// `vitest related` ignores the ones outside the module graph.
const vitestSourcePattern = /^(src|scripts|tests)\//;

// Formatter and linter inputs: a change here can affect every file.
const formatWidePattern =
  /^(package(-lock)?\.json$|\.prettierrc[^/]*$|prettier\.config\.[cm]?js$|\.prettierignore$|\.editorconfig$)/;
const lintWidePattern =
  /^(package(-lock)?\.json$|eslint\.config\.[cm]?js$|tsconfig[^/]*\.json$)/;

// What the Node budget tests read: the scripts themselves and the workflow,
// manifest and package files their contract tests parse.
const nodeTestInputPattern =
  /^(scripts\/|\.github\/workflows\/|Cargo\.toml$|src-tauri\/Cargo\.toml$|package(-lock)?\.json$)/;

const lintablePattern = /\.(?:[cm]?[jt]sx?)$/;
const typeInputPattern = /\.(?:[cm]?[jt]sx?|json)$/;

// `files` lists both sides of every rename and may name deleted files: a
// deletion still decides the lanes, but only files that `exists` are checked.
// A file list is "all" when the change can affect files it does not name.
export function changeScope(files, exists = () => true) {
  const paths = [...new Set(files.map((file) => file.replaceAll("\\", "/")))];
  const present = paths.filter((file) => exists(file));
  const touches = (pattern) => paths.some((file) => pattern.test(file));
  return {
    rust: touches(rustPathPattern),
    nodeTests: touches(nodeTestInputPattern),
    // A deleted module can break its importers, so deletions count here.
    typescript: touches(typeInputPattern),
    format: touches(formatWidePattern) ? "all" : present,
    lint: touches(lintWidePattern)
      ? "all"
      : present.filter((file) => lintablePattern.test(file)),
    vitest: touches(vitestWidePattern)
      ? "all"
      : present.filter((file) => vitestSourcePattern.test(file)),
  };
}

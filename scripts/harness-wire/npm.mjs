// The npm registry calls contract providers make. A Windows shell joins
// arguments unquoted, so a path that may hold a space is quoted there.
import { execFileSync } from "node:child_process";
import path from "node:path";

const shell = process.platform === "win32";

// A field of a package's newest published release.
export function published(name, field = "version") {
  return JSON.parse(
    execFileSync("npm", ["view", name, field, "--json"], {
      encoding: "utf8",
      shell,
    }),
  );
}

// Installs one published package (no scripts, no platform binaries) into
// `work` and returns its folder.
export function installed(spec, work) {
  execFileSync(
    "npm",
    [
      "install",
      "--ignore-scripts",
      "--omit=optional",
      "--no-audit",
      "--no-fund",
      "--prefix",
      shell ? `"${work}"` : work,
      spec,
    ],
    { shell, stdio: ["ignore", "ignore", "inherit"] },
  );
  return path.join(work, "node_modules", spec.slice(0, spec.lastIndexOf("@")));
}

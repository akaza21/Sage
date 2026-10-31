import { readFileSync } from "node:fs";

// The Cargo features of src-tauri that build only on some platforms. Each
// one's crate code refuses to compile elsewhere with `compile_error!`, so
// `--all-features` cannot build on any platform and every Cargo gate names
// its features instead. Every other feature builds everywhere.
export const platformFeatures = Object.freeze({
  // Drives headless Chromium over CDP (src/browser/devtools/mod.rs).
  "cdp-lane": Object.freeze(["darwin", "linux"]),
  // Boots WebView2, Windows debug builds only (src/lib.rs).
  "native-lane": Object.freeze(["win32"]),
});

// Device lanes run through their own runners (`npm run test:cdp-lane`,
// `npm run test:native-lane`), which provision the browser they drive. Lint
// covers them, but the cargo test phases leave them out.
export const deviceLaneFeatures = Object.freeze(Object.keys(platformFeatures));

const manifestUrl = new URL("../src-tauri/Cargo.toml", import.meta.url);

export function declaredCargoFeatures(
  manifest = readFileSync(manifestUrl, "utf8"),
) {
  const section = manifest.match(
    /^\[features\]\r?\n([\s\S]*?)(?=^\[|(?![\s\S]))/m,
  );
  if (!section) throw new Error("src-tauri/Cargo.toml declares no [features]");
  return section[1]
    .split(/\r?\n/)
    .map((line) => line.match(/^([A-Za-z0-9_-]+)\s*=/)?.[1])
    .filter((name) => name !== undefined && name !== "default")
    .sort();
}

function buildsOn(feature, platform) {
  const platforms = platformFeatures[feature];
  return platforms === undefined || platforms.includes(platform);
}

// Every feature that builds on `platform`: what Clippy lints.
export function lintFeatures(
  platform = process.platform,
  declared = declaredCargoFeatures(),
) {
  return declared.filter((feature) => buildsOn(feature, platform));
}

// What the cargo test phases and the fixture build use on `platform`.
export function testFeatures(
  platform = process.platform,
  declared = declaredCargoFeatures(),
) {
  return lintFeatures(platform, declared).filter(
    (feature) => !deviceLaneFeatures.includes(feature),
  );
}

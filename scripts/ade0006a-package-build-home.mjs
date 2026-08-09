import { lstat, mkdir, writeFile } from "node:fs/promises";
import path from "node:path";

async function assertCargoHomeClean(cargoHome) {
  for (const filename of ["config", "config.toml"]) {
    try {
      await lstat(path.join(cargoHome, filename));
      throw new Error("cargo-home-configuration-forbidden");
    } catch (error) {
      if (error.code !== "ENOENT") throw error;
    }
  }
}

export async function prepareIsolatedPackageBuildHome({ root, cargoHome }) {
  await assertCargoHomeClean(cargoHome);
  const value = {
    profileRoot: path.join(root, "profile"),
    npmCache: path.join(root, "npm-cache"),
    viteCache: path.join(root, "vite-cache"),
    npmUserConfig: path.join(root, "npm-user.ini"),
    npmGlobalConfig: path.join(root, "npm-global.ini"),
    metadataTarget: path.join(root, "metadata-target"),
  };
  await mkdir(root, { recursive: false });
  await Promise.all([
    mkdir(path.join(value.profileRoot, "Temp"), { recursive: true }),
    mkdir(path.join(value.profileRoot, "AppData", "Roaming"), {
      recursive: true,
    }),
    mkdir(path.join(value.profileRoot, "AppData", "Local"), {
      recursive: true,
    }),
    mkdir(value.npmCache, { recursive: false }),
    mkdir(value.viteCache, { recursive: false }),
    mkdir(value.metadataTarget, { recursive: false }),
  ]);
  await Promise.all([
    writeFile(path.join(value.profileRoot, "Temp", ".sage-build-home"), "1", {
      flag: "wx",
    }),
    writeFile(value.npmUserConfig, "", { flag: "wx" }),
    writeFile(value.npmGlobalConfig, "", { flag: "wx" }),
  ]);
  return value;
}

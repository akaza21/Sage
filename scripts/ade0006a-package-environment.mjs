import path from "node:path";

function absolute(value, label) {
  if (typeof value !== "string" || !path.isAbsolute(value)) {
    throw new Error(`package environment ${label} must be absolute`);
  }
  return path.resolve(value);
}

function baseEnvironment(context) {
  const systemRoot = absolute(context.systemRoot, "SystemRoot");
  const profileRoot = absolute(context.profileRoot, "profile root");
  const temp = path.join(profileRoot, "Temp");
  return {
    APPDATA: path.join(profileRoot, "AppData", "Roaming"),
    CI: "true",
    ComSpec: absolute(context.comSpec, "ComSpec"),
    HOME: profileRoot,
    LOCALAPPDATA: path.join(profileRoot, "AppData", "Local"),
    NO_PROXY: "*",
    PATHEXT: ".COM;.EXE;.BAT;.CMD",
    SystemRoot: systemRoot,
    TEMP: temp,
    TMP: temp,
    USERPROFILE: profileRoot,
    WINDIR: systemRoot,
  };
}

function executableDirectories(context) {
  const msvcToolsRoot = absolute(context.msvcToolsRoot, "MSVC tools root");
  const windowsSdkRoot = absolute(context.windowsSdkRoot, "Windows SDK root");
  const windowsSdkVersion = context.windowsSdkVersion;
  if (!/^\d+\.\d+\.\d+\.\d+$/.test(windowsSdkVersion ?? "")) {
    throw new Error("package environment Windows SDK version is invalid");
  }
  const msvcBin = path.join(msvcToolsRoot, "bin", "Hostx64", "x64");
  const sdkBin = path.join(windowsSdkRoot, "bin", windowsSdkVersion, "x64");
  return {
    msvcToolsRoot,
    windowsSdkRoot,
    windowsSdkVersion,
    msvcBin,
    sdkBin,
    path: [
      path.dirname(absolute(context.cargo, "Cargo")),
      path.dirname(absolute(context.node, "Node")),
      path.dirname(absolute(context.git, "Git")),
      msvcBin,
      sdkBin,
      path.join(absolute(context.systemRoot, "SystemRoot"), "System32"),
    ].join(";"),
  };
}

export function createPackageNpmEnvironment(context) {
  const directories = executableDirectories(context);
  return {
    ...baseEnvironment(context),
    PATH: directories.path,
    SAGE_VITE_CACHE_DIR: absolute(context.viteCache, "Vite cache"),
    npm_config_cache: absolute(context.npmCache, "npm cache"),
    npm_config_fund: "false",
    npm_config_globalconfig: absolute(
      context.npmGlobalConfig,
      "npm global config",
    ),
    npm_config_ignore_scripts: "true",
    npm_config_proxy: "",
    npm_config_update_notifier: "false",
    npm_config_userconfig: absolute(context.npmUserConfig, "npm user config"),
  };
}

export function createPackageGitEnvironment(context) {
  const directories = executableDirectories(context);
  return {
    ...baseEnvironment(context),
    GIT_CONFIG_GLOBAL: "NUL",
    GIT_CONFIG_NOSYSTEM: "1",
    GIT_OPTIONAL_LOCKS: "0",
    PATH: directories.path,
  };
}

export function createPackageCargoEnvironment(context, targetDirectory) {
  const directories = executableDirectories(context);
  const sdkInclude = path.join(
    directories.windowsSdkRoot,
    "Include",
    directories.windowsSdkVersion,
  );
  const sdkLib = path.join(
    directories.windowsSdkRoot,
    "Lib",
    directories.windowsSdkVersion,
  );
  return {
    ...baseEnvironment(context),
    PATH: directories.path,
    CARGO: absolute(context.cargo, "Cargo"),
    CARGO_HOME: absolute(context.cargoHome, "Cargo home"),
    CARGO_INCREMENTAL: "0",
    CARGO_NET_OFFLINE: "true",
    CARGO_TARGET_DIR: absolute(targetDirectory, "Cargo target"),
    CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER: path.join(
      directories.msvcBin,
      "link.exe",
    ),
    CARGO_TERM_COLOR: "never",
    CC_x86_64_pc_windows_msvc: path.join(directories.msvcBin, "cl.exe"),
    CXX_x86_64_pc_windows_msvc: path.join(directories.msvcBin, "cl.exe"),
    INCLUDE: [
      path.join(directories.msvcToolsRoot, "include"),
      path.join(sdkInclude, "ucrt"),
      path.join(sdkInclude, "shared"),
      path.join(sdkInclude, "um"),
      path.join(sdkInclude, "winrt"),
      path.join(sdkInclude, "cppwinrt"),
    ].join(";"),
    LIB: [
      path.join(directories.msvcToolsRoot, "lib", "x64"),
      path.join(sdkLib, "ucrt", "x64"),
      path.join(sdkLib, "um", "x64"),
    ].join(";"),
    LIBPATH: path.join(directories.msvcToolsRoot, "lib", "x64"),
    RUSTC: absolute(context.rustc, "Rustc"),
  };
}

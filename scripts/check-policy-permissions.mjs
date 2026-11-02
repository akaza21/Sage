// The WebView only subscribes to backend events; every other core plugin
// command (windows, webviews, paths, menus, emit) stays out of reach.
const allowedCorePermissions = new Set([
  "core:event:allow-listen",
  "core:event:allow-unlisten",
]);

export function validateMainCapabilityPermissions(capability) {
  const permissions = (capability.permissions ?? []).map((permission) =>
    typeof permission === "string" ? permission : permission.identifier,
  );
  for (const identifier of permissions) {
    if (allowedCorePermissions.has(identifier)) continue;
    if (
      identifier === "main-commands" &&
      capability.identifier === "main" &&
      capability.windows === undefined &&
      capability.remote === undefined &&
      JSON.stringify(capability.webviews) === JSON.stringify(["main"])
    )
      continue;
    throw new Error(`Unexpected Tauri permission: ${identifier}`);
  }
}

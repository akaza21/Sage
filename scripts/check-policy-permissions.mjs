export function validateMainCapabilityPermissions(capability) {
  const permissions = (capability.permissions ?? []).map((permission) =>
    typeof permission === "string" ? permission : permission.identifier,
  );
  for (const identifier of permissions) {
    if (identifier === "core:default") continue;
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

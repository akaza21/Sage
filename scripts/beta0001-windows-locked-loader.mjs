const bootstrapEnvironmentName = "SAGE_INSTALLED_SMOKE_BOOTSTRAP";
const bootstrapMaximumBytes = 256 * 1024;

const lockedLoader = String.raw`
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
$environmentName = "SAGE_INSTALLED_SMOKE_BOOTSTRAP"
$encoded = [Environment]::GetEnvironmentVariable($environmentName, "Process")
if ([string]::IsNullOrWhiteSpace($encoded)) {
  $serialized = [Console]::In.ReadToEnd()
} else {
  [Environment]::SetEnvironmentVariable($environmentName, $null, "Process")
  $serialized = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($encoded))
}
if ([Text.Encoding]::UTF8.GetByteCount($serialized) -gt 262144) { throw "Locked bootstrap exceeds its bound." }
$request = $serialized | ConvertFrom-Json
if ([int]$request.schemaVersion -ne 1 -or [string]::IsNullOrWhiteSpace([string]$request.sourceRoot) -or
    [string]::IsNullOrWhiteSpace([string]$request.entryRelativePath) -or
    $request.identities.Count -lt 1 -or $request.identities.Count -gt 32) {
  throw "Locked bootstrap request is invalid."
}
$root = [IO.Path]::GetFullPath([string]$request.sourceRoot).TrimEnd('\')
$volumeRoot = [IO.Path]::GetPathRoot($root)
if ([string]::IsNullOrWhiteSpace($volumeRoot) -or $root -eq $volumeRoot.TrimEnd('\')) { throw "Locked source root is ambiguous." }
$streams = [Collections.Generic.List[IO.FileStream]]::new()
$entrySource = $null
try {
  foreach ($identity in $request.identities) {
    $relative = [string]$identity.relativePath
    if ($relative -notmatch '^[A-Za-z0-9._/-]+$' -or $relative.Contains('..') -or
        [long]$identity.bytes -lt 1 -or [long]$identity.bytes -gt 2097152 -or
        [string]$identity.sha256 -notmatch '^[0-9a-f]{64}$') { throw "Locked file identity is invalid." }
    $fullPath = [IO.Path]::GetFullPath((Join-Path $root $relative.Replace('/', '\')))
    if (-not $fullPath.StartsWith("$root\", [StringComparison]::OrdinalIgnoreCase)) { throw "Locked file escaped source root." }
    $current = $volumeRoot
    foreach ($segment in $fullPath.Substring($volumeRoot.Length).Split('\')) {
      $current = Join-Path $current $segment
      $item = Get-Item -LiteralPath $current -Force -ErrorAction Stop
      if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Locked file path contains a reparse point." }
    }
    $stream = [IO.FileStream]::new($fullPath, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    $streams.Add($stream)
    if ($stream.Length -ne [long]$identity.bytes) { throw "Locked file size changed." }
    $hasher = [Security.Cryptography.SHA256]::Create()
    try { $hash = [BitConverter]::ToString($hasher.ComputeHash($stream)).Replace('-', '').ToLowerInvariant() }
    finally { $hasher.Dispose() }
    if ($hash -ne [string]$identity.sha256) { throw "Locked file hash changed." }
    if ($relative -eq [string]$request.entryRelativePath) {
      $stream.Position = 0
      $reader = [IO.StreamReader]::new($stream, [Text.UTF8Encoding]::new($false, $true), $true, 4096, $true)
      try { $entrySource = $reader.ReadToEnd() } finally { $reader.Dispose() }
    }
  }
  if ($null -eq $entrySource) { throw "Locked entrypoint is absent from the closure." }
  $parameters = @{}
  foreach ($property in $request.parameters.PSObject.Properties) { $parameters[$property.Name] = $property.Value }
  & ([ScriptBlock]::Create($entrySource)) @parameters
} finally {
  foreach ($stream in $streams) { $stream.Dispose() }
}
`;

const lockedLoaderEncodedCommand = Buffer.from(
  lockedLoader,
  "utf16le",
).toString("base64");

export function serializeLockedPowerShellRequest({
  sourceRoot,
  entryRelativePath,
  identities,
  parameters,
}) {
  const request = Buffer.from(
    JSON.stringify({
      schemaVersion: 1,
      sourceRoot,
      entryRelativePath,
      identities: identities.map(({ relativePath, sha256, bytes }) => ({
        relativePath,
        sha256,
        bytes,
      })),
      parameters,
    }),
  );
  if (request.byteLength > bootstrapMaximumBytes) {
    throw new Error("installed-smoke-locked-bootstrap-oversize");
  }
  return request;
}

export function lockedPowerShellArguments() {
  return [
    "-NoLogo",
    "-NoProfile",
    "-NonInteractive",
    "-ExecutionPolicy",
    "Bypass",
    "-EncodedCommand",
    lockedLoaderEncodedCommand,
  ];
}

export const INSTALLED_SMOKE_BOOTSTRAP_ENVIRONMENT_NAME =
  bootstrapEnvironmentName;
export const INSTALLED_SMOKE_LOCKED_LOADER_ENCODED_COMMAND =
  lockedLoaderEncodedCommand;
export const INSTALLED_SMOKE_LOCKED_BOOTSTRAP_MAXIMUM_BYTES =
  bootstrapMaximumBytes;

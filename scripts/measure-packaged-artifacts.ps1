[CmdletBinding()]
param(
  [Parameter(Mandatory)]
  [string]$SageExecutablePath,

  [Parameter(Mandatory)]
  [string]$FrontendDirectory,

  [Parameter(Mandatory)]
  [string]$BundleDirectory,

  [string]$OutputPath = "",

  [string]$InstalledDirectory,

  [string]$ControlExecutablePath
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if ([string]::IsNullOrWhiteSpace($OutputPath)) {
  $workspaceRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
  $stamp = [DateTime]::UtcNow.ToString("yyyyMMdd-HHmmss")
  $OutputPath = Join-Path $workspaceRoot (
    "local-evidence/windows-footprint/$stamp-artifacts.json"
  )
} elseif (-not [IO.Path]::IsPathRooted($OutputPath)) {
  throw "OutputPath must be absolute."
}

function Resolve-RequiredFile {
  param([Parameter(Mandatory)][string]$Path)

  if (-not [IO.Path]::IsPathRooted($Path)) {
    throw "Expected an absolute file path: $Path"
  }
  if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
    throw "Required file does not exist: $Path"
  }
  return (Resolve-Path -LiteralPath $Path).Path
}

function Resolve-RequiredDirectory {
  param([Parameter(Mandatory)][string]$Path)

  if (-not [IO.Path]::IsPathRooted($Path)) {
    throw "Expected an absolute directory path: $Path"
  }
  if (-not (Test-Path -LiteralPath $Path -PathType Container)) {
    throw "Required directory does not exist: $Path"
  }
  return (Resolve-Path -LiteralPath $Path).Path
}

function Get-FileEvidence {
  param([Parameter(Mandatory)][IO.FileInfo]$File)

  return [ordered]@{
    path = $File.FullName
    bytes = [long]$File.Length
    sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $File.FullName).Hash
  }
}

function Get-ByteSum {
  param(
    [AllowEmptyCollection()]
    [object[]]$Items
  )

  $sum = 0L
  foreach ($item in $Items) {
    $sum += [long]$item.Length
  }
  return $sum
}

function Get-DirectoryEvidence {
  param([Parameter(Mandatory)][string]$Path)

  $files = @(
    Get-ChildItem -LiteralPath $Path -Recurse -File |
      Sort-Object FullName
  )
  return [ordered]@{
    path = $Path
    fileCount = $files.Count
    bytes = Get-ByteSum $files
    files = @($files | ForEach-Object { Get-FileEvidence $_ })
  }
}

$sageExecutable = Get-Item -LiteralPath (
  Resolve-RequiredFile $SageExecutablePath
)
$frontend = Resolve-RequiredDirectory $FrontendDirectory
$bundle = Resolve-RequiredDirectory $BundleDirectory
$frontendFiles = @(Get-ChildItem -LiteralPath $frontend -Recurse -File)
$javascript = @($frontendFiles | Where-Object Extension -eq ".js")
$css = @($frontendFiles | Where-Object Extension -eq ".css")
$maps = @($frontendFiles | Where-Object Extension -eq ".map")

$evidence = [ordered]@{
  schemaVersion = 1
  generatedAtUtc = [DateTime]::UtcNow.ToString("o")
  sageExecutable = Get-FileEvidence $sageExecutable
  installedCorePayloadBytes = [long]$sageExecutable.Length
  installedCoreDefinition = (
    "The custom-protocol executable embeds all frontend assets; WebView2 is " +
    "a shared OS runtime and no sidecar or resource is bundled."
  )
  frontend = [ordered]@{
    directory = $frontend
    allBytes = Get-ByteSum $frontendFiles
    fileCount = $frontendFiles.Count
    javascriptBytes = Get-ByteSum $javascript
    cssBytes = Get-ByteSum $css
    sourceMapBytes = Get-ByteSum $maps
    sourceMapCount = $maps.Count
    files = @(
      $frontendFiles |
        Sort-Object FullName |
        ForEach-Object { Get-FileEvidence $_ }
    )
  }
  bundles = Get-DirectoryEvidence $bundle
  installedDirectory = if ([string]::IsNullOrWhiteSpace($InstalledDirectory)) {
    $null
  } else {
    Get-DirectoryEvidence (
      Resolve-RequiredDirectory $InstalledDirectory
    )
  }
  controlExecutable = if (
    [string]::IsNullOrWhiteSpace($ControlExecutablePath)
  ) {
    $null
  } else {
    Get-FileEvidence (
      Get-Item -LiteralPath (
        Resolve-RequiredFile $ControlExecutablePath
      )
    )
  }
}

$parent = Split-Path -Parent $OutputPath
if (-not (Test-Path -LiteralPath $parent)) {
  [void](New-Item -ItemType Directory -Force -Path $parent)
}
$evidence | ConvertTo-Json -Depth 8 |
  Set-Content -LiteralPath $OutputPath -Encoding UTF8
$evidence | ConvertTo-Json -Depth 8

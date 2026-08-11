Set-StrictMode -Version Latest

function Get-InstalledSmokeStringSha256 {
  param([Parameter(Mandatory)][string]$Value)
  $hasher = [Security.Cryptography.SHA256]::Create()
  try {
    return ([BitConverter]::ToString($hasher.ComputeHash([Text.Encoding]::UTF8.GetBytes($Value)))).Replace("-", "").ToLowerInvariant()
  } finally { $hasher.Dispose() }
}

function Set-InstalledSmokeSemanticStep {
  param(
    [Parameter(Mandatory)]$State,
    [Parameter(Mandatory)][string]$Id,
    [Parameter(Mandatory)][Diagnostics.Stopwatch]$Timer,
    [Parameter(Mandatory)]$Evidence
  )
  $index = [Array]::IndexOf([string[]]$State.stepIds, $Id)
  $State.steps[$index] = [ordered]@{
    id = $Id; status = "passed"
    durationMs = [Math]::Min(600000, [Math]::Max(0, [int]$Timer.ElapsedMilliseconds))
    evidenceSha256 = Get-InstalledSmokeStringSha256 ($Evidence | ConvertTo-Json -Compress -Depth 6)
  }
}

function Get-InstalledSmokeAvailablePort {
  $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
  try {
    $listener.Start()
    return ([Net.IPEndPoint]$listener.LocalEndpoint).Port
  } finally { $listener.Stop() }
}

function Invoke-InstalledSmokeSemanticCampaign {
  param(
    [Parameter(Mandatory)]$State,
    [Parameter(Mandatory)][scriptblock]$IdentityGate,
    [Parameter(Mandatory)][string]$LaunchStep,
    [Parameter(Mandatory)][string]$DriveStep,
    [Parameter(Mandatory)][string]$CloseStep,
    [Parameter(Mandatory)][ValidateSet("primary", "restart", "reinstall")][string]$Mode,
    [Parameter(Mandatory)]$Executable,
    [Parameter(Mandatory)]$Config
  )
  $State.currentStep = $LaunchStep
  & $IdentityGate
  $launchTimer = [Diagnostics.Stopwatch]::StartNew()
  $launchIdentity = Assert-InstalledSmokeFileIdentity $Executable.path $Config.outputRoot ([long]$Executable.bytes) ([string]$Executable.sha256)
  $port = Get-InstalledSmokeAvailablePort
  $start = [Diagnostics.ProcessStartInfo]::new()
  $start.FileName = $launchIdentity.path
  $start.WorkingDirectory = $Config.installRoot
  $start.UseShellExecute = $false
  $start.CreateNoWindow = $true
  foreach ($name in @(
    "SAGE_PROJECT_ROOT", "SAGE_DIAGNOSTIC_DATA_DIR",
    "SAGE_CHARACTERIZATION_FIXTURE_PATH", "SAGE_CHARACTERIZATION_FIXTURE_SHA256",
    "SAGE_CHARACTERIZATION_FIXTURE_BYTES", "SAGE_CHARACTERIZATION_CASE"
  )) { [void]$start.EnvironmentVariables.Remove($name) }
  $start.EnvironmentVariables["SAGE_DISPOSABLE_PROFILE_ROOT"] = $Config.profileRoot
  if ($Mode -eq "primary") { $start.EnvironmentVariables["SAGE_PROJECT_ROOT"] = $Config.fixtureRoot }
  else { [void]$start.EnvironmentVariables.Remove("SAGE_PROJECT_ROOT") }
  $start.EnvironmentVariables["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = "--remote-debugging-port=$port"
  $start.EnvironmentVariables["WEBVIEW2_BROWSER_EXECUTABLE_FOLDER"] = ""
  $start.EnvironmentVariables["PATH"] = "{0};{1}" -f ([IO.Path]::GetDirectoryName($Config.gitPath)), (Join-Path $env:SystemRoot "System32")
  $start.EnvironmentVariables["GIT_CONFIG_NOSYSTEM"] = "1"
  $start.EnvironmentVariables["GIT_CONFIG_GLOBAL"] = "NUL"
  $State.job = Start-FootprintJobProcess $start
  $primaryFailure = $null
  $primaryFailurePhase = $null
  try {
    $window = Wait-FootprintWindow $State.job.process.Id 20
    Set-InstalledSmokeSemanticStep $State $LaunchStep $launchTimer ([ordered]@{
      windowTitleSha256 = Get-InstalledSmokeStringSha256 $window.title
    })
    $State.currentStep = $DriveStep
    $driveTimer = [Diagnostics.Stopwatch]::StartNew()
    $semanticPath = Join-Path $Config.outputRoot ("semantic-{0}.json" -f $Mode)
    if (Test-Path -LiteralPath $semanticPath) { throw "Semantic evidence path is not fresh." }
    $invokeBounded = $Config.invokeBounded
    $nodeResult = & $invokeBounded $Config.nodePath @($Config.driver, [string]$port, $Mode, $Config.rootLabel, $semanticPath) "drive-$Mode" 60000
    $semantic = Get-Content -LiteralPath $semanticPath -Raw | ConvertFrom-Json
    if ((Get-Item -LiteralPath $semanticPath).Length -gt 32768 -or
        [int]$semantic.schemaVersion -ne 1 -or [string]$semantic.mode -ne $Mode) {
      throw "Semantic evidence is invalid."
    }
    foreach ($name in $State.assertionNames) {
      if ([bool]$semantic.assertions.$name) { $State.assertions[$name] = $true }
    }
    Set-InstalledSmokeSemanticStep $State $DriveStep $driveTimer ([ordered]@{
      stdoutSha256 = Get-InstalledSmokeStringSha256 $nodeResult.stdout
      semanticSha256 = (Get-FileHash $semanticPath -Algorithm SHA256).Hash.ToLowerInvariant()
    })
  } catch {
    $primaryFailure = $_
    $primaryFailurePhase = $State.currentStep
  } finally {
    $State.currentStep = $CloseStep
    $closeTimer = [Diagnostics.Stopwatch]::StartNew()
    if ($null -ne $State.job) {
      try {
        $teardown = Stop-FootprintJobProcess $State.job.process $State.job.boundary 1000 5000
        if (-not $teardown.treeBoundaryVerified -or $teardown.orphanProcessesAfterClose -ne 0) {
          throw "Launch cleanup identity was not verified."
        }
        $State.job.boundary.Dispose()
        $State.job = $null
        Set-InstalledSmokeSemanticStep $State $CloseStep $closeTimer ([ordered]@{
          forced = [bool]$teardown.forced; zeroOrphans = $true
        })
      } catch {
        $primaryFailure = $_
        $primaryFailurePhase = $CloseStep
      }
    }
    try { & $IdentityGate } catch {
      if ($null -eq $primaryFailure) {
        $primaryFailure = $_
        $primaryFailurePhase = $DriveStep
      }
    }
  }
  if ($null -ne $primaryFailure) {
    $State.currentStep = $primaryFailurePhase
    throw $primaryFailure
  }
}

function New-InstalledSmokeFixtureRepository {
  param(
    [Parameter(Mandatory)][string]$FixtureRoot,
    [Parameter(Mandatory)][string]$RootLabel,
    [Parameter(Mandatory)][scriptblock]$InvokeGit
  )
  [void][IO.Directory]::CreateDirectory($FixtureRoot)
  [IO.File]::WriteAllText((Join-Path $FixtureRoot "README.md"), "sage installed smoke initial`n", [Text.UTF8Encoding]::new($false))
  foreach ($command in @(
    @("init"), @("config", "user.name", "Sage Smoke"),
    @("config", "user.email", "sage-smoke@example.invalid"),
    @("add", "README.md"), @("commit", "-m", "initial fixture")
  )) { [void](& $InvokeGit ([string[]](@("-C", $FixtureRoot) + $command))) }
  $head = (& $InvokeGit ([string[]]@("-C", $FixtureRoot, "rev-parse", "HEAD"))).stdout.Trim()
  return [ordered]@{
    rootIdentitySha256 = Get-InstalledSmokeStringSha256 ([IO.Path]::GetFullPath($FixtureRoot).TrimEnd('\').ToLowerInvariant())
    rootLabel = $RootLabel
    initialCommitSha256 = Get-InstalledSmokeStringSha256 $head
  }
}

Export-ModuleMember -Function @(
  "Invoke-InstalledSmokeSemanticCampaign",
  "New-InstalledSmokeFixtureRepository"
)

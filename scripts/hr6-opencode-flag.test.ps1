param(
  [Parameter(Mandatory)][string]$LauncherPath,
  [ValidateRange(1000, 600000)][int]$TimeoutMs = 6000,
  [ValidateRange(1000, 600000)][int]$OuterDeadlineMs = 120000
)

$ErrorActionPreference = "Stop"

$killGraceMs = 5000
$exitGraceMs = 5000
$blockingDeadlineMs = 9000
$grandchildPollStepMs = 250
$grandchildPollMs = 5000
$grandchildReadyPollStepMs = 100
$grandchildReadyMs = 5000
$grandchildRecheckMs = 300
$grandchildHandshakeMs = $grandchildReadyMs + $grandchildRecheckMs
$fixedOverheadMs = 10000
$mappingInvocations = 4
$timeoutOverheadMs = $killGraceMs + $exitGraceMs
$composedWorstCaseMs = ($mappingInvocations * ($TimeoutMs + $timeoutOverheadMs)) + $blockingDeadlineMs + $timeoutOverheadMs + $grandchildPollMs + $fixedOverheadMs
if ($composedWorstCaseMs -ge $OuterDeadlineMs) { throw "Composed worst case $composedWorstCaseMs ms must stay under the outer deadline $OuterDeadlineMs ms." }
if ($grandchildHandshakeMs -ge $blockingDeadlineMs) { throw "Grandchild handshake budget $grandchildHandshakeMs ms must complete before the blocking deadline $blockingDeadlineMs ms." }

$failures = New-Object System.Collections.Generic.List[string]

function Add-Failure([string]$Message) { $failures.Add($Message) }

function Format-PathArgument([string]$Value) {
  if ($Value.Contains('"')) { throw "Paths containing double quotes are unsupported: $Value" }
  return '"' + $Value + '"'
}

function Stop-BoundedTree([Diagnostics.Process]$Target, [int]$Deadline) {
  $killTree = $Target.GetType().GetMethod("Kill", [type[]]@([bool]))
  if ($null -ne $killTree) {
    $treeKilled = $false
    try {
      $killTree.Invoke($Target, @($true))
      $treeKilled = $true
    } catch { }
    if ($treeKilled) { return }
  }
  $killerOut = $null
  $killerErr = $null
  $killer = $null
  try {
    $killerOut = [IO.Path]::GetTempFileName()
    $killerErr = [IO.Path]::GetTempFileName()
    $killer = Start-Process -FilePath "taskkill.exe" -ArgumentList @("/PID", $Target.Id, "/T", "/F") -RedirectStandardOutput $killerOut -RedirectStandardError $killerErr -NoNewWindow -PassThru
    $null = $killer.Handle
    if (-not $killer.WaitForExit($Deadline)) { try { $killer.Kill() } catch { } }
  } catch { } finally {
    if ($null -ne $killer) { $killer.Dispose() }
    if ($null -ne $killerOut -and [IO.File]::Exists($killerOut)) { [IO.File]::Delete($killerOut) }
    if ($null -ne $killerErr -and [IO.File]::Exists($killerErr)) { [IO.File]::Delete($killerErr) }
  }
}

function Invoke-Bounded([string]$Path, [bool]$Requested, [int]$Deadline) {
  $quoted = Format-PathArgument $Path
  $arguments = @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $quoted, "-SourceRoot", $quoted, "-SageExecutable", $quoted)
  if ($Requested) { $arguments += "-IncludeOpenCode" }
  $priorPreference = $ErrorActionPreference
  $ErrorActionPreference = "Continue"
  $outPath = $null
  $errPath = $null
  $process = $null
  try {
    $outPath = [IO.Path]::GetTempFileName()
    $errPath = [IO.Path]::GetTempFileName()
    $process = Start-Process -FilePath "powershell.exe" -ArgumentList $arguments -RedirectStandardOutput $outPath -RedirectStandardError $errPath -NoNewWindow -PassThru
    $null = $process.Handle
    if (-not $process.WaitForExit($Deadline)) {
      Stop-BoundedTree $process $killGraceMs
      try { if (-not $process.HasExited) { $process.Kill() } } catch { }
      $null = $process.WaitForExit($exitGraceMs)
      return [ordered]@{ exitCode = -1; output = ""; timedOut = $true; killed = $process.HasExited }
    }
    $text = "" + (Get-Content -LiteralPath $outPath -Raw -ErrorAction SilentlyContinue) + (Get-Content -LiteralPath $errPath -Raw -ErrorAction SilentlyContinue)
    return [ordered]@{ exitCode = $process.ExitCode; output = $text.Trim(); timedOut = $false; killed = $false }
  } finally {
    if ($null -ne $process) { $process.Dispose() }
    if ($null -ne $outPath -and [IO.File]::Exists($outPath)) { [IO.File]::Delete($outPath) }
    if ($null -ne $errPath -and [IO.File]::Exists($errPath)) { [IO.File]::Delete($errPath) }
    $ErrorActionPreference = $priorPreference
  }
}

function New-Fixture([string[]]$Body, [string]$Root) {
  $path = Join-Path $Root ("hr6-opencode-flag-" + [Guid]::NewGuid().ToString("n") + ".ps1")
  [IO.File]::WriteAllText($path, ($Body -join [Environment]::NewLine) + [Environment]::NewLine, [Text.UTF8Encoding]::new($false))
  return $path
}

$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($LauncherPath, [ref]$tokens, [ref]$errors)
if ($errors.Count -gt 0) { throw "Launcher does not parse: $($errors[0].Message)" }
if ($null -eq $ast.ParamBlock) { throw "Launcher has no parameter block." }

$invocations = @($ast.FindAll({ param($node) $node -is [Management.Automation.Language.CommandAst] }, $true)) | Where-Object { @($_.CommandElements | Where-Object { $_.Extent.Text -eq "--include-opencode" }).Count -eq 1 }
if (@($invocations).Count -ne 1) { throw "Expected exactly one command passing --include-opencode, found $(@($invocations).Count)." }

$elements = @($invocations)[0].CommandElements
$flagIndex = -1
for ($index = 0; $index -lt $elements.Count; $index++) {
  if ($elements[$index].Extent.Text -eq "--include-opencode") { $flagIndex = $index }
}
if ($flagIndex -lt 0 -or $flagIndex + 1 -ge $elements.Count) { throw "--include-opencode has no value argument." }
$flagElement = $elements[$flagIndex + 1]
if ($flagElement -isnot [Management.Automation.Language.VariableExpressionAst]) { throw "--include-opencode value is not a variable: $($flagElement.Extent.Text)" }
$flagName = $flagElement.VariablePath.UserPath

$assignments = @(@($ast.FindAll({ param($node) $node -is [Management.Automation.Language.AssignmentStatementAst] }, $true)) | Where-Object { $_.Left -is [Management.Automation.Language.VariableExpressionAst] -and $_.Left.VariablePath.UserPath -eq $flagName })
if ($assignments.Count -lt 1) { throw "No assignment computes `$$flagName." }

$paramBlock = $ast.ParamBlock.Extent.Text

$mapping = New-Object System.Collections.Generic.List[string]
$mapping.Add($paramBlock)
foreach ($assignment in $assignments) { $mapping.Add($assignment.Extent.Text) }
$mapping.Add("Write-Output ('flag=' + `$$flagName)")

function Test-Mapping([string[]]$Body, [string]$Root, [string]$Label, [int]$Deadline) {
  $fixture = New-Fixture $Body $Root
  try {
    $requested = Invoke-Bounded $fixture $true $Deadline
    $default = Invoke-Bounded $fixture $false $Deadline
    if ($requested.timedOut) { Add-Failure "-IncludeOpenCode from $Label should map within $Deadline ms but was killed at the deadline." }
    elseif ($requested.exitCode -ne 0) { Add-Failure "-IncludeOpenCode from $Label should bind and map cleanly but exited $($requested.exitCode): $($requested.output)" }
    elseif ($requested.output -ne "flag=true") { Add-Failure "-IncludeOpenCode from $Label should pass flag=true but passed '$($requested.output)'." }
    if ($default.timedOut) { Add-Failure "The default run from $Label should map within $Deadline ms but was killed at the deadline." }
    elseif ($default.exitCode -ne 0) { Add-Failure "The default run from $Label should bind and map cleanly but exited $($default.exitCode): $($default.output)" }
    elseif ($default.output -ne "flag=false") { Add-Failure "The default run from $Label should pass flag=false but passed '$($default.output)'." }
  } finally {
    if ([IO.File]::Exists($fixture)) { [IO.File]::Delete($fixture) }
  }
}

$spacedRoot = Join-Path ([IO.Path]::GetTempPath()) ("hr6 opencode flag " + [Guid]::NewGuid().ToString("n"))
try {
  [void][IO.Directory]::CreateDirectory($spacedRoot)
  Test-Mapping $mapping ([IO.Path]::GetTempPath()) "the default temp root" $TimeoutMs
  Test-Mapping $mapping $spacedRoot "a root containing spaces" $TimeoutMs
} finally {
  if ([IO.Directory]::Exists($spacedRoot)) { [IO.Directory]::Delete($spacedRoot, $true) }
}

$grandchildPidFile = Join-Path ([IO.Path]::GetTempPath()) ("hr6-opencode-flag-pid-" + [Guid]::NewGuid().ToString("n") + ".txt")
$grandchildReadyFile = Join-Path ([IO.Path]::GetTempPath()) ("hr6-opencode-flag-ready-" + [Guid]::NewGuid().ToString("n") + ".txt")

$grandchildHandshake = @'
$grandchildCommand = '[IO.File]::WriteAllText($env:HR6_GRANDCHILD_READY_FILE, [string]$PID); Start-Sleep -Seconds 300'
$grandchild = Start-Process -FilePath 'powershell.exe' -ArgumentList @('-NoProfile', '-Command', '"' + $grandchildCommand + '"') -WindowStyle Hidden -PassThru
$readyWatch = [Diagnostics.Stopwatch]::StartNew()
$readyPid = 0
while ($readyWatch.ElapsedMilliseconds -lt %READYMS%) {
  if ([IO.File]::Exists($env:HR6_GRANDCHILD_READY_FILE)) {
    $readyText = ('' + [IO.File]::ReadAllText($env:HR6_GRANDCHILD_READY_FILE)).Trim()
    if ($readyText -match '^\d+$') { $readyPid = [int]$readyText; break }
  }
  Start-Sleep -Milliseconds %STEPMS%
}
$readyWatch.Stop()
if ($readyPid -eq $grandchild.Id) {
  $alive = Get-Process -Id $grandchild.Id -ErrorAction SilentlyContinue
  if ($null -ne $alive) {
    Start-Sleep -Milliseconds %RECHECKMS%
    $alive = Get-Process -Id $grandchild.Id -ErrorAction SilentlyContinue
    if ($null -ne $alive) {
      [IO.File]::WriteAllText($env:HR6_GRANDCHILD_PID_FILE, [string]$alive.Id + '|' + [string]$alive.StartTime.Ticks)
    }
  }
}
'@
$grandchildHandshake = $grandchildHandshake.Replace('%READYMS%', [string]$grandchildReadyMs).Replace('%STEPMS%', [string]$grandchildReadyPollStepMs).Replace('%RECHECKMS%', [string]$grandchildRecheckMs)

$blocking = New-Object System.Collections.Generic.List[string]
$blocking.Add($paramBlock)
$blocking.Add($grandchildHandshake)
$blocking.Add("`$$flagName = `$(Start-Sleep -Seconds 300; 'true')")
$blocking.Add("Write-Output ('flag=' + `$$flagName)")

$blockingFixture = New-Fixture $blocking ([IO.Path]::GetTempPath())
$blockingDeadline = $blockingDeadlineMs
$blocked = $null
$grandchildId = 0
$grandchildTicks = 0
$priorPidFile = $env:HR6_GRANDCHILD_PID_FILE
$priorReadyFile = $env:HR6_GRANDCHILD_READY_FILE
$env:HR6_GRANDCHILD_PID_FILE = $grandchildPidFile
$env:HR6_GRANDCHILD_READY_FILE = $grandchildReadyFile
$watch = [Diagnostics.Stopwatch]::StartNew()
try {
  $blocked = Invoke-Bounded $blockingFixture $true $blockingDeadline
} finally {
  $watch.Stop()
  $env:HR6_GRANDCHILD_PID_FILE = $priorPidFile
  $env:HR6_GRANDCHILD_READY_FILE = $priorReadyFile
  if ([IO.File]::Exists($grandchildPidFile)) {
    $recorded = ("" + [IO.File]::ReadAllText($grandchildPidFile)).Trim()
    $recordedParts = $recorded.Split('|')
    if ($recordedParts.Count -eq 2 -and $recordedParts[0] -match '^\d+$' -and $recordedParts[1] -match '^\d+$') {
      $grandchildId = [int]$recordedParts[0]
      $grandchildTicks = [long]$recordedParts[1]
    }
    [IO.File]::Delete($grandchildPidFile)
  }
  if ([IO.File]::Exists($grandchildReadyFile)) { [IO.File]::Delete($grandchildReadyFile) }
  if ([IO.File]::Exists($blockingFixture)) { [IO.File]::Delete($blockingFixture) }
}
if ($null -eq $blocked -or -not $blocked.timedOut) { Add-Failure "A blocking assignment should trip the $blockingDeadline ms deadline but reported exitCode $($blocked.exitCode) output '$($blocked.output)'." }
elseif (-not $blocked.killed) { Add-Failure "A blocking assignment should be terminated at the deadline but the process was still alive." }
if ($watch.ElapsedMilliseconds -ge 60000) { Add-Failure "The deadline should bound a blocking assignment far below a minute but took $($watch.ElapsedMilliseconds) ms." }
if ([IO.File]::Exists($blockingFixture)) { Add-Failure "The fixture should be removed after a timeout but $blockingFixture remains." }
if ($grandchildId -le 0 -or $grandchildTicks -le 0) { Add-Failure "The blocking fixture should confirm a live grandchild process before the deadline and record its id and start time but recorded '$grandchildId|$grandchildTicks'." }
else {
  $survivor = $null
  for ($attempt = 0; $attempt -lt ($grandchildPollMs / $grandchildPollStepMs); $attempt++) {
    $survivor = $null
    $candidate = Get-Process -Id $grandchildId -ErrorAction SilentlyContinue
    if ($null -ne $candidate) {
      $candidateTicks = 0
      try { $candidateTicks = $candidate.StartTime.Ticks } catch { $candidateTicks = 0 }
      if ($candidateTicks -eq $grandchildTicks) { $survivor = $candidate }
    }
    if ($null -eq $survivor) { break }
    Start-Sleep -Milliseconds $grandchildPollStepMs
  }
  if ($null -ne $survivor) {
    Add-Failure "The deadline should kill grandchild process $grandchildId but it survived."
    Stop-Process -Id $grandchildId -Force -ErrorAction SilentlyContinue
  }
}

if ($failures.Count -gt 0) {
  foreach ($failure in $failures) { Write-Output $failure }
  exit 1
}
Write-Output "hr6-opencode-flag checks passed"

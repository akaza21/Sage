Set-StrictMode -Version Latest

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

$script:DebugEnvironmentNames = @(
  "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
  "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER"
)

function Get-FootprintScenarioContract {
  param([Parameter(Mandatory)][ValidateSet("chat", "explorer")][string]$Scenario)

  return [ordered]@{
    scenario = $Scenario
    setupRequired = $true
    setupVerificationRequired = $true
    measuredVerificationRequired = $true
    chatCenterRequired = $true
    explorerSidebarRequired = $Scenario -eq "explorer"
    exactTitle = "Sage"
    exactOrigin = "http://tauri.localhost/"
  }
}

function Assert-FootprintDebugStateClear {
  param([AllowNull()][object[]]$ProcessRows)

  foreach ($name in $script:DebugEnvironmentNames) {
    if (-not [string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($name, "Process"))) {
      throw "Acceptance refuses the inherited $name variable."
    }
  }
  foreach ($row in @($ProcessRows)) {
    if ($row.commandLine -and [string]$row.commandLine -match "--remote-debugging-(?:port|pipe)") {
      throw "A measured descendant retained WebView2 debugging arguments."
    }
  }
}

function Test-FootprintCandidateBinding {
  param(
    [Parameter(Mandatory)][string]$ExecutablePath,
    [Parameter(Mandatory)][string]$ExpectedExecutableSha256,
    [Parameter(Mandatory)][string]$ExpectedSourceAggregateSha256,
    [Parameter(Mandatory)][string]$CandidateBindingPath
  )

  if (-not (Test-Path -LiteralPath $CandidateBindingPath -PathType Leaf)) {
    throw "Candidate binding file is missing."
  }
  $actual = (Get-FileHash -LiteralPath $ExecutablePath -Algorithm SHA256).Hash.ToLowerInvariant()
  $expectedExecutable = $ExpectedExecutableSha256.ToLowerInvariant()
  $expectedSource = $ExpectedSourceAggregateSha256.ToLowerInvariant()
  if ($actual -ne $expectedExecutable) { throw "Candidate executable hash mismatch." }
  $binding = Get-Content -LiteralPath $CandidateBindingPath -Raw | ConvertFrom-Json
  if (([string]$binding.candidate.executableSha256).ToLowerInvariant() -ne $expectedExecutable) {
    throw "Binding file does not identify the exact candidate executable."
  }
  if (([string]$binding.candidate.aggregateSha256).ToLowerInvariant() -ne $expectedSource) {
    throw "Binding file does not identify the exact source aggregate."
  }
  return [ordered]@{
    executableSha256 = $expectedExecutable
    sourceAggregateSha256 = $expectedSource
    bindingPath = (Resolve-Path -LiteralPath $CandidateBindingPath).Path
  }
}

function Get-FootprintScenarioUiaEvidence {
  param(
    [Parameter(Mandatory)][IntPtr]$WindowHandle,
    [Parameter(Mandatory)][int]$ExpectedProcessId,
    [Parameter(Mandatory)][ValidateSet("chat", "explorer")][string]$Scenario
  )

  $contract = Get-FootprintScenarioContract $Scenario
  $root = [Windows.Automation.AutomationElement]::FromHandle($WindowHandle)
  if ($null -eq $root) { throw "UI Automation did not expose the exact Sage window." }
  if ([int]$root.Current.ProcessId -ne $ExpectedProcessId -or $root.Current.Name -ne $contract.exactTitle) {
    throw "UI Automation root PID/title does not identify the measured Sage process."
  }
  $walker = [Windows.Automation.TreeWalker]::ControlViewWalker
  $current = $walker.GetFirstChild($root)
  $visited = 0
  $named = 0
  $chatCount = 0
  $explorerCount = 0
  while ($null -ne $current) {
    $visited += 1
    if ($visited -gt 4096) { throw "UI Automation traversal exceeded 4,096 elements." }
    $name = [string]$current.Current.Name
    if (-not [string]::IsNullOrWhiteSpace($name)) {
      $named += 1
      if ($name.Trim() -eq "Agent chat") { $chatCount += 1 }
      if ($name.Trim() -eq "Project files") { $explorerCount += 1 }
    }
    $child = $walker.GetFirstChild($current)
    if ($null -ne $child) { $current = $child; continue }
    while ($null -ne $current) {
      $sibling = $walker.GetNextSibling($current)
      if ($null -ne $sibling) { $current = $sibling; break }
      $parent = $walker.GetParent($current)
      if ($null -eq $parent -or $parent.Equals($root)) { $current = $null; break }
      $current = $parent
    }
  }
  if ($chatCount -ne 1) { throw "Measured UIA tree does not contain exactly one Chat center." }
  if ($contract.explorerSidebarRequired -and $explorerCount -ne 1) {
    throw "Measured UIA tree does not contain exactly one active Explorer sidebar."
  }
  return [ordered]@{
    scenario = $Scenario
    semanticVerified = $true
    rootProcessId = $ExpectedProcessId
    rootTitle = [string]$root.Current.Name
    visitedElementCount = $visited
    namedElementCount = $named
    chatCenterCount = $chatCount
    explorerSidebarCount = $explorerCount
  }
}

function Wait-FootprintScenarioUiaEvidence {
  param(
    [Parameter(Mandatory)][IntPtr]$WindowHandle,
    [Parameter(Mandatory)][int]$ExpectedProcessId,
    [Parameter(Mandatory)][ValidateSet("chat", "explorer")][string]$Scenario,
    [ValidateRange(1, 30)][int]$TimeoutSeconds = 10
  )
  $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
  do {
    try {
      return Get-FootprintScenarioUiaEvidence $WindowHandle $ExpectedProcessId $Scenario
    } catch {
      $lastError = $_.Exception.Message
      Start-Sleep -Milliseconds 100
    }
  } while ([DateTime]::UtcNow -lt $deadline)
  throw "Measured semantic state was not proven: $lastError"
}

function Assert-FootprintScenarioAcceptance {
  param(
    [Parameter(Mandatory)][ValidateSet("chat", "explorer")][string]$Scenario,
    [Parameter(Mandatory)][Collections.IDictionary]$Setup,
    [Parameter(Mandatory)][Collections.IDictionary]$Measured,
    [Parameter(Mandatory)][object[]]$Samples,
    [Parameter(Mandatory)][string]$ExpectedExecutablePath,
    [Parameter(Mandatory)][string]$ExpectedExecutableSha256,
    [Parameter(Mandatory)][string]$ExpectedSourceAggregateSha256
  )

  $contract = Get-FootprintScenarioContract $Scenario
  foreach ($name in @("executed", "semanticVerified", "processId", "title", "origin", "executablePath", "executableSha256", "sourceAggregateSha256")) {
    if (-not $Setup.Contains($name)) { throw "Setup evidence is missing '$name'." }
  }
  foreach ($name in @("semanticVerified", "processId", "rootTitle", "chatCenterCount", "explorerSidebarCount")) {
    if (-not $Measured.Contains($name)) { throw "Measured evidence is missing '$name'." }
  }
  if (-not $Setup.executed -or -not $Setup.semanticVerified -or $Setup.title -ne $contract.exactTitle -or $Setup.origin -ne $contract.exactOrigin) {
    throw "Instrumented scenario setup was not semantically verified."
  }
  if (-not [string]::Equals([IO.Path]::GetFullPath($Setup.executablePath), [IO.Path]::GetFullPath($ExpectedExecutablePath), [StringComparison]::OrdinalIgnoreCase)) {
    throw "Setup used a different executable path."
  }
  if ($Setup.executableSha256 -ne $ExpectedExecutableSha256 -or $Setup.sourceAggregateSha256 -ne $ExpectedSourceAggregateSha256) {
    throw "Setup candidate/source binding does not match acceptance inputs."
  }
  if ([int]$Setup.processId -eq [int]$Measured.processId) { throw "Setup and measured launches must be distinct processes." }
  if (-not $Measured.semanticVerified -or $Measured.rootTitle -ne $contract.exactTitle -or [int]$Measured.chatCenterCount -ne 1) {
    throw "Measured Chat center was not proven through UI Automation."
  }
  if ($contract.explorerSidebarRequired -and [int]$Measured.explorerSidebarCount -ne 1) {
    throw "Measured Explorer sidebar was not proven through UI Automation."
  }
  foreach ($sample in $Samples) {
    if ([int]$sample.unknownProcessCount -ne 0) { throw "Measured tree contains an unknown descendant." }
    Assert-FootprintDebugStateClear @($sample.processes)
  }
  return $true
}

Export-ModuleMember -Function @(
  "Assert-FootprintDebugStateClear",
  "Assert-FootprintScenarioAcceptance",
  "Get-FootprintScenarioContract",
  "Get-FootprintScenarioUiaEvidence",
  "Test-FootprintCandidateBinding",
  "Wait-FootprintScenarioUiaEvidence"
)

Set-StrictMode -Version Latest
Import-Module (Join-Path $PSScriptRoot "ProcessIdentity.psm1")

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

$script:MaximumTraversalElements = 4096

function Get-C0BUiaControlType {
  param([Parameter(Mandatory)][string]$Name)
  $types = @{
    button = [Windows.Automation.ControlType]::Button
    checkbox = [Windows.Automation.ControlType]::CheckBox
    document = [Windows.Automation.ControlType]::Document
    edit = [Windows.Automation.ControlType]::Edit
    group = [Windows.Automation.ControlType]::Group
    hyperlink = [Windows.Automation.ControlType]::Hyperlink
    listitem = [Windows.Automation.ControlType]::ListItem
    pane = [Windows.Automation.ControlType]::Pane
    tabitem = [Windows.Automation.ControlType]::TabItem
    text = [Windows.Automation.ControlType]::Text
    tree = [Windows.Automation.ControlType]::Tree
    treeitem = [Windows.Automation.ControlType]::TreeItem
  }
  if (-not $types.ContainsKey($Name)) { throw "Unsupported UI Automation control type '$Name'." }
  return $types[$Name]
}

function Get-C0BSemanticWindow {
  param(
    [Parameter(Mandatory)][IntPtr]$WindowHandle,
    [Parameter(Mandatory)][ValidateRange(1, [int]::MaxValue)][int]$ExpectedProcessId,
    [Parameter(Mandatory)][ValidateLength(1, 128)][string]$ExpectedTitle
  )
  if ($WindowHandle -eq [IntPtr]::Zero) { throw "Semantic UI Automation requires a nonzero window handle." }
  $root = [Windows.Automation.AutomationElement]::FromHandle($WindowHandle)
  if ($null -eq $root -or [int]$root.Current.ProcessId -ne $ExpectedProcessId -or
      -not [string]::Equals([string]$root.Current.Name, $ExpectedTitle, [StringComparison]::Ordinal)) {
    throw "UI Automation root does not identify the exact process, window, and title."
  }
  return $root
}

function Get-C0BAllowedProcessIds {
  param(
    [Parameter(Mandatory)][int]$ExpectedRootProcessId,
    [Parameter(Mandatory)][string]$ExpectedExecutablePath,
    [Parameter(Mandatory)][string]$ExpectedCreationTimeUtc
  )
  $rows = @(Get-FootprintProcessSnapshot)
  [void](Get-FootprintRootIdentity $ExpectedRootProcessId $ExpectedExecutablePath $ExpectedCreationTimeUtc $rows)
  return @(Get-FootprintDescendants $ExpectedRootProcessId $rows | ForEach-Object { [int]$_.ProcessId })
}

function Find-C0BSemanticElement {
  param(
    [Parameter(Mandatory)][Windows.Automation.AutomationElement]$Root,
    [Parameter(Mandatory)][ValidateCount(1, 128)][int[]]$AllowedProcessIds,
    [AllowNull()][string]$Name,
    [AllowNull()][string]$AutomationId,
    [AllowNull()][ValidateSet("button", "checkbox", "document", "edit", "group", "hyperlink", "listitem", "pane", "tabitem", "text", "tree", "treeitem")][string]$ControlType,
    [ValidateSet("control", "raw")][string]$View = "control",
    [ValidateRange(1, 16)][int]$MaximumMatches = 2
  )
  if ([string]::IsNullOrEmpty($Name) -and [string]::IsNullOrEmpty($AutomationId)) {
    throw "Semantic UI Automation requires an exact accessible name or automation ID."
  }
  if ([Text.Encoding]::UTF8.GetByteCount([string]$Name) -gt 512 -or
      [Text.Encoding]::UTF8.GetByteCount([string]$AutomationId) -gt 512) {
    throw "Semantic UI Automation selector exceeds 512 bytes."
  }
  $allowed = [Collections.Generic.HashSet[int]]::new()
  foreach ($processId in $AllowedProcessIds) {
    if ($processId -lt 1 -or -not $allowed.Add($processId)) {
      throw "Semantic UI Automation process ownership is invalid."
    }
  }
  $expectedType = if ($ControlType) { Get-C0BUiaControlType $ControlType } else { $null }
  $walker = if ($View -eq "control") {
    [Windows.Automation.TreeWalker]::ControlViewWalker
  } else {
    [Windows.Automation.TreeWalker]::RawViewWalker
  }
  $matches = [Collections.Generic.List[object]]::new($MaximumMatches)
  $current = $walker.GetFirstChild($Root)
  $visited = 0
  while ($null -ne $current) {
    $visited += 1
    if ($visited -gt $script:MaximumTraversalElements) {
      throw "UI Automation traversal exceeded $($script:MaximumTraversalElements) elements."
    }
    if ($allowed.Contains([int]$current.Current.ProcessId)) {
      $nameMatches = [string]::IsNullOrEmpty($Name) -or
        [string]::Equals([string]$current.Current.Name, $Name, [StringComparison]::Ordinal)
      $idMatches = [string]::IsNullOrEmpty($AutomationId) -or
        [string]::Equals([string]$current.Current.AutomationId, $AutomationId, [StringComparison]::Ordinal)
      $typeMatches = $null -eq $expectedType -or $current.Current.ControlType -eq $expectedType
      if ($nameMatches -and $idMatches -and $typeMatches) {
        if ($matches.Count -ge $MaximumMatches) { throw "UI Automation selector exceeded its match bound." }
        $matches.Add($current)
      }
    }
    $child = $walker.GetFirstChild($current)
    if ($null -ne $child) { $current = $child; continue }
    while ($null -ne $current) {
      $sibling = $walker.GetNextSibling($current)
      if ($null -ne $sibling) { $current = $sibling; break }
      $parent = $walker.GetParent($current)
      if ($null -eq $parent -or $parent.Equals($Root)) { $current = $null; break }
      $current = $parent
    }
  }
  return [ordered]@{ elements = $matches.ToArray(); visited = $visited; view = $View }
}

function Get-C0BSemanticElement {
  param(
    [Parameter(Mandatory)][Windows.Automation.AutomationElement]$Root,
    [Parameter(Mandatory)][ValidateCount(1, 128)][int[]]$AllowedProcessIds,
    [AllowNull()][string]$Name,
    [AllowNull()][string]$AutomationId,
    [AllowNull()][string]$ControlType,
    [ValidateSet("control", "raw")][string]$View = "control"
  )
  $found = Find-C0BSemanticElement $Root $AllowedProcessIds $Name $AutomationId $ControlType $View 2
  if ($found.elements.Count -ne 1) { throw "Semantic selector must identify exactly one UI Automation element." }
  return $found.elements[0]
}

function Wait-C0BSemanticElement {
  param(
    [Parameter(Mandatory)][IntPtr]$WindowHandle,
    [Parameter(Mandatory)][int]$ExpectedRootProcessId,
    [Parameter(Mandatory)][string]$ExpectedTitle,
    [Parameter(Mandatory)][string]$ExpectedExecutablePath,
    [Parameter(Mandatory)][string]$ExpectedCreationTimeUtc,
    [AllowNull()][string]$Name,
    [AllowNull()][string]$AutomationId,
    [AllowNull()][string]$ControlType,
    [ValidateSet("control", "raw")][string]$View = "control",
    [ValidateRange(100, 30000)][int]$TimeoutMs = 10000
  )
  $timer = [Diagnostics.Stopwatch]::StartNew()
  do {
    try {
      $root = Get-C0BSemanticWindow $WindowHandle $ExpectedRootProcessId $ExpectedTitle
      $allowedProcessIds = @(Get-C0BAllowedProcessIds `
        $ExpectedRootProcessId $ExpectedExecutablePath $ExpectedCreationTimeUtc)
      if ($ExpectedRootProcessId -notin $allowedProcessIds) {
        throw "Semantic UI Automation ownership lost the exact native root."
      }
      return Get-C0BSemanticElement $root $allowedProcessIds $Name $AutomationId $ControlType $View
    } catch {
      $lastError = $_.Exception.Message
      Start-Sleep -Milliseconds 50
    }
  } while ($timer.ElapsedMilliseconds -lt $TimeoutMs)
  throw "Semantic UI Automation selector timed out: $lastError"
}

function Invoke-C0BSemanticAction {
  param(
    [Parameter(Mandatory)][Windows.Automation.AutomationElement]$Element,
    [Parameter(Mandatory)][ValidateSet("focus", "invoke", "select", "set-value", "toggle")][string]$Action,
    [AllowNull()][string]$Value
  )
  if ($Action -eq "focus") { $Element.SetFocus(); return }
  $pattern = $null
  if ($Action -eq "invoke") {
    if (-not $Element.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
      throw "Semantic element does not support Invoke."
    }
    ([Windows.Automation.InvokePattern]$pattern).Invoke()
    return
  }
  if ($Action -eq "select") {
    if (-not $Element.TryGetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern, [ref]$pattern)) {
      throw "Semantic element does not support SelectionItem."
    }
    ([Windows.Automation.SelectionItemPattern]$pattern).Select()
    return
  }
  if ($Action -eq "toggle") {
    if (-not $Element.TryGetCurrentPattern([Windows.Automation.TogglePattern]::Pattern, [ref]$pattern)) {
      throw "Semantic element does not support Toggle."
    }
    ([Windows.Automation.TogglePattern]$pattern).Toggle()
    return
  }
  if ($null -eq $Value -or [Text.Encoding]::UTF8.GetByteCount($Value) -gt 32768) {
    throw "Semantic value must contain at most 32,768 UTF-8 bytes."
  }
  if (-not $Element.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
    throw "Semantic element does not support Value."
  }
  ([Windows.Automation.ValuePattern]$pattern).SetValue($Value)
}

function Wait-C0BSemanticValue {
  param(
    [Parameter(Mandatory)][IntPtr]$WindowHandle,
    [Parameter(Mandatory)][int]$ExpectedRootProcessId,
    [Parameter(Mandatory)][string]$ExpectedTitle,
    [Parameter(Mandatory)][string]$ExpectedExecutablePath,
    [Parameter(Mandatory)][string]$ExpectedCreationTimeUtc,
    [AllowNull()][string]$Name,
    [AllowNull()][string]$AutomationId,
    [AllowNull()][string]$ControlType,
    [Parameter(Mandatory)][ValidateLength(0, 32768)][string]$ExpectedValue,
    [ValidateSet("control", "raw")][string]$View = "control",
    [ValidateRange(100, 30000)][int]$TimeoutMs = 10000
  )
  if ([Text.Encoding]::UTF8.GetByteCount($ExpectedValue) -gt 32768) {
    throw "Semantic expected value exceeds 32,768 UTF-8 bytes."
  }
  $timer = [Diagnostics.Stopwatch]::StartNew()
  do {
    try {
      $root = Get-C0BSemanticWindow $WindowHandle $ExpectedRootProcessId $ExpectedTitle
      $allowedProcessIds = @(Get-C0BAllowedProcessIds `
        $ExpectedRootProcessId $ExpectedExecutablePath $ExpectedCreationTimeUtc)
      if ($ExpectedRootProcessId -notin $allowedProcessIds) {
        throw "Semantic UI Automation ownership lost the exact native root."
      }
      $element = Get-C0BSemanticElement $root $allowedProcessIds $Name $AutomationId $ControlType $View
      $pattern = $null
      if ($element.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern, [ref]$pattern) -and
          [string]::Equals(([Windows.Automation.ValuePattern]$pattern).Current.Value, $ExpectedValue, [StringComparison]::Ordinal)) {
        return $element
      }
      $lastError = "Semantic value has not reached its exact expected value."
    } catch {
      $lastError = $_.Exception.Message
    }
    Start-Sleep -Milliseconds 20
  } while ($timer.ElapsedMilliseconds -lt $TimeoutMs)
  throw "Semantic UI Automation value timed out: $lastError"
}

Export-ModuleMember -Function @(
  "Find-C0BSemanticElement",
  "Get-C0BSemanticElement",
  "Get-C0BSemanticWindow",
  "Invoke-C0BSemanticAction",
  "Wait-C0BSemanticElement",
  "Wait-C0BSemanticValue"
)

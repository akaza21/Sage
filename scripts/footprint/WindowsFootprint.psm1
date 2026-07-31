Set-StrictMode -Version Latest

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

if (-not ("SageFootprint.NativeMethods" -as [type])) {
  Add-Type -ReferencedAssemblies System.Drawing @'
using System;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Imaging;
using System.IO;
using System.Runtime.InteropServices;
using System.Security.Cryptography;

namespace SageFootprint {
  public static class NativeMethods {
    [DllImport("user32.dll")]
    private static extern bool SetForegroundWindow(IntPtr window);

    [DllImport("user32.dll")]
    private static extern bool ShowWindowAsync(IntPtr window, int command);

    [DllImport("user32.dll")]
    private static extern bool SetWindowPos(
      IntPtr window,
      IntPtr insertAfter,
      int x,
      int y,
      int width,
      int height,
      uint flags
    );

    [DllImport("user32.dll")]
    private static extern bool SetCursorPos(int x, int y);

    [DllImport("user32.dll")]
    private static extern IntPtr WindowFromPoint(POINT point);

    [DllImport("user32.dll")]
    private static extern IntPtr GetAncestor(IntPtr window, uint flags);

    [DllImport("user32.dll")]
    private static extern bool IsChild(IntPtr parent, IntPtr window);

    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(
      IntPtr window,
      out uint processId
    );

    [DllImport("user32.dll")]
    private static extern void mouse_event(
      uint flags,
      uint dx,
      uint dy,
      uint data,
      UIntPtr extraInfo
    );

    [DllImport("user32.dll")]
    private static extern uint SendInput(
      uint inputCount,
      INPUT[] inputs,
      int inputSize
    );

    [StructLayout(LayoutKind.Sequential)]
    private struct INPUT {
      public uint type;
      public INPUTUNION data;
    }

    [StructLayout(LayoutKind.Explicit)]
    private struct INPUTUNION {
      [FieldOffset(0)]
      public MOUSEINPUT mouse;

      [FieldOffset(0)]
      public KEYBDINPUT keyboard;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct MOUSEINPUT {
      public int dx;
      public int dy;
      public uint mouseData;
      public uint flags;
      public uint time;
      public UIntPtr extraInfo;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct KEYBDINPUT {
      public ushort virtualKey;
      public ushort scanCode;
      public uint flags;
      public uint time;
      public UIntPtr extraInfo;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct POINT {
      public int x;
      public int y;
    }

    private const uint MouseLeftDown = 0x0002;
    private const uint MouseLeftUp = 0x0004;
    private const uint InputKeyboard = 1;
    private const uint KeyUp = 0x0002;
    private const uint Unicode = 0x0004;
    private const int RestoreWindow = 9;
    private const uint RootAncestor = 2;
    private const uint RootOwnerAncestor = 3;
    private const uint NoMove = 0x0002;
    private const uint NoSize = 0x0001;
    private const uint ShowWindow = 0x0040;
    private static readonly IntPtr TopMost = new IntPtr(-1);
    private static readonly IntPtr NotTopMost = new IntPtr(-2);

    public static void BringToFront(IntPtr window) {
      ShowWindowAsync(window, RestoreWindow);
      SetWindowPos(
        window,
        TopMost,
        0,
        0,
        0,
        0,
        NoMove | NoSize | ShowWindow
      );
      SetForegroundWindow(window);
      System.Threading.Thread.Sleep(75);
      SetWindowPos(window, NotTopMost, 0, 0, 0, 0, NoMove | NoSize);
      SetForegroundWindow(window);
    }

    public static void PinToFront(IntPtr window) {
      ShowWindowAsync(window, RestoreWindow);
      SetWindowPos(
        window,
        TopMost,
        0,
        0,
        0,
        0,
        NoMove | NoSize | ShowWindow
      );
      SetForegroundWindow(window);
      System.Threading.Thread.Sleep(75);
    }

    public static void FocusAndClick(IntPtr window, int x, int y) {
      BringToFront(window);
      ClickVisible(window, x, y);
    }

    public static void ClickVisible(IntPtr window, int x, int y) {
      if (!SetCursorPos(x, y)) {
        throw new InvalidOperationException("Could not position the pointer.");
      }
      POINT point = new POINT { x = x, y = y };
      IntPtr pointedWindow = WindowFromPoint(point);
      IntPtr pointedRoot = GetAncestor(pointedWindow, RootAncestor);
      IntPtr pointedOwner = GetAncestor(pointedWindow, RootOwnerAncestor);
      if (
        pointedRoot != window &&
        pointedOwner != window &&
        !IsChild(window, pointedWindow)
      ) {
        uint pointedProcessId;
        GetWindowThreadProcessId(pointedRoot, out pointedProcessId);
        throw new InvalidOperationException(
          "The target window was not visually foremost at the click point " +
          "(target=" + window + ", pointed=" + pointedRoot +
          ", owner=" + pointedOwner +
          ", pointedPid=" + pointedProcessId + ")."
        );
      }
      mouse_event(MouseLeftDown, 0, 0, 0, UIntPtr.Zero);
      mouse_event(MouseLeftUp, 0, 0, 0, UIntPtr.Zero);
    }

    public static void SendUnicode(char value) {
      INPUT[] inputs = new INPUT[2];
      inputs[0].type = InputKeyboard;
      inputs[0].data.keyboard.scanCode = value;
      inputs[0].data.keyboard.flags = Unicode;
      inputs[1].type = InputKeyboard;
      inputs[1].data.keyboard.scanCode = value;
      inputs[1].data.keyboard.flags = Unicode | KeyUp;
      if (SendInput(2, inputs, Marshal.SizeOf(typeof(INPUT))) != 2) {
        throw new InvalidOperationException("Could not inject Unicode input.");
      }
    }

    public static void PressVirtualKey(ushort virtualKey) {
      INPUT[] inputs = new INPUT[2];
      inputs[0].type = InputKeyboard;
      inputs[0].data.keyboard.virtualKey = virtualKey;
      inputs[1].type = InputKeyboard;
      inputs[1].data.keyboard.virtualKey = virtualKey;
      inputs[1].data.keyboard.flags = KeyUp;
      if (SendInput(2, inputs, Marshal.SizeOf(typeof(INPUT))) != 2) {
        throw new InvalidOperationException("Could not inject virtual-key input.");
      }
    }

    public static void PressChord(ushort modifier, ushort virtualKey) {
      INPUT[] inputs = new INPUT[4];
      inputs[0].type = InputKeyboard;
      inputs[0].data.keyboard.virtualKey = modifier;
      inputs[1].type = InputKeyboard;
      inputs[1].data.keyboard.virtualKey = virtualKey;
      inputs[2].type = InputKeyboard;
      inputs[2].data.keyboard.virtualKey = virtualKey;
      inputs[2].data.keyboard.flags = KeyUp;
      inputs[3].type = InputKeyboard;
      inputs[3].data.keyboard.virtualKey = modifier;
      inputs[3].data.keyboard.flags = KeyUp;
      if (SendInput(4, inputs, Marshal.SizeOf(typeof(INPUT))) != 4) {
        throw new InvalidOperationException("Could not inject key chord.");
      }
    }

    public static string CaptureHash(int x, int y, int width, int height) {
      if (width < 1 || height < 1) {
        throw new ArgumentOutOfRangeException("Capture dimensions must be positive.");
      }
      using (Bitmap bitmap = new Bitmap(width, height, PixelFormat.Format32bppArgb))
      using (Graphics graphics = Graphics.FromImage(bitmap))
      using (MemoryStream stream = new MemoryStream())
      using (SHA256 sha = SHA256.Create()) {
        graphics.CopyFromScreen(x, y, 0, 0, new Size(width, height));
        bitmap.Save(stream, ImageFormat.Bmp);
        return Convert.ToBase64String(sha.ComputeHash(stream.ToArray()));
      }
    }

    public static void CapturePng(
      int x,
      int y,
      int width,
      int height,
      string path
    ) {
      using (Bitmap bitmap = new Bitmap(width, height, PixelFormat.Format32bppArgb))
      using (Graphics graphics = Graphics.FromImage(bitmap)) {
        graphics.CopyFromScreen(x, y, 0, 0, new Size(width, height));
        bitmap.Save(path, ImageFormat.Png);
      }
    }
  }
}
'@
}

function Get-FootprintMedian {
  param([Parameter(Mandatory)][double[]]$Values)

  if ($Values.Count -eq 0) {
    throw "Median requires at least one value."
  }

  $ordered = @($Values | Sort-Object)
  $middle = [Math]::Floor($ordered.Count / 2)
  if ($ordered.Count % 2 -eq 1) {
    return [double]$ordered[$middle]
  }

  return ([double]$ordered[$middle - 1] + [double]$ordered[$middle]) / 2
}

function Get-FootprintPercentile {
  param(
    [Parameter(Mandatory)][double[]]$Values,
    [Parameter(Mandatory)][ValidateRange(0.000001, 1)][double]$Percentile
  )

  if ($Values.Count -eq 0) {
    throw "Percentile requires at least one value."
  }

  $ordered = @($Values | Sort-Object)
  $rank = [Math]::Ceiling($Percentile * $ordered.Count)
  return [double]$ordered[[Math]::Max(0, $rank - 1)]
}

function Get-FootprintProcessRole {
  param(
    [Parameter(Mandatory)][string]$Name,
    [AllowNull()][string]$CommandLine,
    [Parameter(Mandatory)][bool]$IsRoot
  )

  if ($IsRoot) {
    return "native-host"
  }

  $normalizedName = [IO.Path]::GetFileNameWithoutExtension($Name).ToLowerInvariant()
  $command = if ($null -eq $CommandLine) { "" } else { $CommandLine }

  if ($normalizedName -eq "msedgewebview2") {
    if ($command -match "(?:^|\s)--type=crashpad-handler(?:\s|$)") {
      return "crashpad"
    }
    if ($command -match "(?:^|\s)--type=renderer(?:\s|$)") {
      return "renderer"
    }
    if ($command -match "(?:^|\s)--type=gpu-process(?:\s|$)") {
      return "gpu"
    }
    if ($command -match "(?:^|\s)--type=utility(?:\s|$)") {
      if ($command -match "network\.mojom\.NetworkService") {
        return "utility-network"
      }
      if ($command -match "storage\.mojom\.StorageService") {
        return "utility-storage"
      }
      return "utility-other"
    }
    if ($command -match "--embedded-browser-webview=1") {
      return "browser"
    }
    return "webview-unknown"
  }

  $excludedNames = @(
    "cmd",
    "conhost",
    "node",
    "openconsole",
    "powershell",
    "pwsh",
    "windowsterminal",
    "wsl",
    "wslhost"
  )
  if ($excludedNames -contains $normalizedName) {
    return "excluded-child"
  }

  return "unknown"
}

function Get-FootprintDescendants {
  param(
    [Parameter(Mandatory)][int]$RootProcessId,
    [AllowNull()][object[]]$ProcessRows
  )

  if ($null -eq $ProcessRows) {
    $ProcessRows = @(Get-CimInstance Win32_Process)
  }

  $included = [Collections.Generic.HashSet[int]]::new()
  [void]$included.Add($RootProcessId)
  $changed = $true

  while ($changed) {
    $changed = $false
    foreach ($row in $ProcessRows) {
      $processId = [int]$row.ProcessId
      $parentId = [int]$row.ParentProcessId
      if (-not $included.Contains($processId) -and $included.Contains($parentId)) {
        [void]$included.Add($processId)
        $changed = $true
      }
    }
  }

  return @(
    $ProcessRows |
      Where-Object { $included.Contains([int]$_.ProcessId) } |
      Sort-Object ProcessId
  )
}

function ConvertTo-FootprintCreationIdentity {
  param([Parameter(Mandatory)]$CreationDate)

  if ($CreationDate -is [DateTime]) {
    return $CreationDate.ToUniversalTime().ToString("o")
  }
  return [string]$CreationDate
}

function Test-FootprintProcessIdentity {
  param(
    [Parameter(Mandatory)]$ProcessRow,
    [Parameter(Mandatory)][string]$ExpectedExecutablePath,
    [Parameter(Mandatory)][string]$ExpectedCreationDate
  )

  if (-not $ProcessRow.ExecutablePath -or -not $ProcessRow.CreationDate) {
    return $false
  }
  $pathMatches = [string]::Equals(
    [IO.Path]::GetFullPath([string]$ProcessRow.ExecutablePath),
    [IO.Path]::GetFullPath($ExpectedExecutablePath),
    [StringComparison]::OrdinalIgnoreCase
  )
  $creationMatches = (
    ConvertTo-FootprintCreationIdentity $ProcessRow.CreationDate
  ) -eq $ExpectedCreationDate
  return $pathMatches -and $creationMatches
}

function Get-FootprintRootIdentity {
  param(
    [Parameter(Mandatory)][int]$RootProcessId,
    [Parameter(Mandatory)][string]$ExpectedExecutablePath,
    [AllowNull()][object[]]$ProcessRows
  )

  if ($null -eq $ProcessRows) {
    $ProcessRows = @(Get-CimInstance Win32_Process)
  }
  $root = @(
    $ProcessRows |
      Where-Object { [int]$_.ProcessId -eq $RootProcessId }
  ) | Select-Object -First 1
  if ($null -eq $root) {
    throw "The launched root process identity could not be captured."
  }
  if (-not $root.ExecutablePath -or -not $root.CreationDate) {
    throw "The launched root lacks executable or creation identity."
  }
  $resolvedExpected = [IO.Path]::GetFullPath($ExpectedExecutablePath)
  if (
    -not [string]::Equals(
      [IO.Path]::GetFullPath([string]$root.ExecutablePath),
      $resolvedExpected,
      [StringComparison]::OrdinalIgnoreCase
    )
  ) {
    throw "The launched root executable identity does not match the target."
  }
  return [ordered]@{
    processId = $RootProcessId
    executablePath = $resolvedExpected
    creationDate = ConvertTo-FootprintCreationIdentity $root.CreationDate
  }
}

function Get-FootprintForceKillCandidates {
  param(
    [Parameter(Mandatory)][Collections.IDictionary]$RootIdentity,
    [AllowNull()][object[]]$ProcessRows
  )

  if ($null -eq $ProcessRows) {
    $ProcessRows = @(Get-CimInstance Win32_Process)
  }
  $rootProcessId = [int]$RootIdentity.processId
  $root = @(
    $ProcessRows |
      Where-Object { [int]$_.ProcessId -eq $rootProcessId }
  ) | Select-Object -First 1
  if (
    $null -eq $root -or
    -not (
      Test-FootprintProcessIdentity `
        -ProcessRow $root `
        -ExpectedExecutablePath ([string]$RootIdentity.executablePath) `
        -ExpectedCreationDate ([string]$RootIdentity.creationDate)
    )
  ) {
    return @()
  }
  return @(
    Get-FootprintDescendants `
      -RootProcessId $rootProcessId `
      -ProcessRows $ProcessRows
  )
}

function Get-FootprintTreeSample {
  param([Parameter(Mandatory)][int]$RootProcessId)

  $cimRows = @(Get-CimInstance Win32_Process)
  $members = @(Get-FootprintDescendants -RootProcessId $RootProcessId -ProcessRows $cimRows)
  $records = @()

  foreach ($member in $members) {
    try {
      $process = Get-Process -Id ([int]$member.ProcessId) -ErrorAction Stop
      $role = Get-FootprintProcessRole `
        -Name ([string]$member.Name) `
        -CommandLine ([string]$member.CommandLine) `
        -IsRoot ([int]$member.ProcessId -eq $RootProcessId)
      $coreIncluded = $role -in @(
        "native-host",
        "browser",
        "renderer",
        "gpu",
        "utility-network",
        "utility-storage",
        "utility-other",
        "crashpad",
        "webview-unknown"
      )

      $records += [ordered]@{
        processId = [int]$member.ProcessId
        parentProcessId = [int]$member.ParentProcessId
        name = [string]$member.Name
        executablePath = if ($member.ExecutablePath) {
          [string]$member.ExecutablePath
        } else {
          $null
        }
        commandLine = if ($member.CommandLine) {
          [string]$member.CommandLine
        } else {
          $null
        }
        role = $role
        coreIncluded = $coreIncluded
        privateBytes = [long]$process.PrivateMemorySize64
        workingSetBytes = [long]$process.WorkingSet64
        cpuSeconds = if ($null -eq $process.CPU) { 0.0 } else { [double]$process.CPU }
      }
    } catch [Microsoft.PowerShell.Commands.ProcessCommandException] {
      # A process may exit between the CIM and Process snapshots. The next
      # bounded sample refreshes membership; no stale process is invented.
    }
  }

  $core = @($records | Where-Object { $_.coreIncluded })
  $excluded = @($records | Where-Object { $_.role -eq "excluded-child" })
  $unknown = @($records | Where-Object { $_.role -in @("unknown", "webview-unknown") })

  return [ordered]@{
    capturedAtUtc = [DateTime]::UtcNow.ToString("o")
    corePrivateBytes = [long](
      ($core | ForEach-Object privateBytes | Measure-Object -Sum).Sum
    )
    coreWorkingSetBytes = [long](
      ($core | ForEach-Object workingSetBytes | Measure-Object -Sum).Sum
    )
    coreCpuSeconds = [double](
      ($core | ForEach-Object cpuSeconds | Measure-Object -Sum).Sum
    )
    excludedPrivateBytes = [long](
      ($excluded | ForEach-Object privateBytes | Measure-Object -Sum).Sum
    )
    excludedWorkingSetBytes = [long](
      ($excluded | ForEach-Object workingSetBytes | Measure-Object -Sum).Sum
    )
    processCount = $records.Count
    unknownProcessCount = $unknown.Count
    processes = $records
  }
}

function Wait-FootprintWindow {
  param(
    [Parameter(Mandatory)][int]$ProcessId,
    [Parameter(Mandatory)][ValidateRange(1, 120)][int]$TimeoutSeconds
  )

  $timer = [Diagnostics.Stopwatch]::StartNew()
  while ($timer.Elapsed.TotalSeconds -lt $TimeoutSeconds) {
    $process = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if ($null -eq $process) {
      throw "Target process exited before its window became available."
    }
    if ($process.MainWindowHandle -ne [IntPtr]::Zero) {
      try {
        [void]$process.WaitForInputIdle(1000)
      } catch {
        # MainWindowHandle is the bounded readiness signal. Some WebView hosts
        # do not support WaitForInputIdle consistently during bootstrap.
      }
      return [ordered]@{
        handle = $process.MainWindowHandle
        elapsedMs = [Math]::Round($timer.Elapsed.TotalMilliseconds, 3)
      }
    }
    Start-Sleep -Milliseconds 20
  }

  throw "Target window did not become available within $TimeoutSeconds seconds."
}

function Get-FootprintWebBounds {
  param(
    [Parameter(Mandatory)][IntPtr]$WindowHandle,
    [ValidateRange(1, 30)][int]$TimeoutSeconds = 10
  )

  $timer = [Diagnostics.Stopwatch]::StartNew()
  while ($timer.Elapsed.TotalSeconds -lt $TimeoutSeconds) {
    $root = [Windows.Automation.AutomationElement]::FromHandle($WindowHandle)
    if ($null -ne $root) {
      $all = $root.FindAll(
        [Windows.Automation.TreeScope]::Descendants,
        [Windows.Automation.Condition]::TrueCondition
      )
      for ($index = 0; $index -lt $all.Count; $index += 1) {
        $element = $all.Item($index)
        if ($element.Current.Name -like "* - Web content") {
          $bounds = $element.Current.BoundingRectangle
          if ($bounds.Width -gt 0 -and $bounds.Height -gt 0) {
            return [ordered]@{
              x = [int][Math]::Round($bounds.X)
              y = [int][Math]::Round($bounds.Y)
              width = [int][Math]::Round($bounds.Width)
              height = [int][Math]::Round($bounds.Height)
            }
          }
        }
      }
    }
    Start-Sleep -Milliseconds 50
  }

  throw (
    "The WebView2 content bounds were not exposed through UI Automation " +
    "within $TimeoutSeconds seconds."
  )
}

function ConvertTo-FootprintScreenPoint {
  param(
    [Parameter(Mandatory)][Collections.IDictionary]$Bounds,
    [Parameter(Mandatory)][double]$CssX,
    [Parameter(Mandatory)][double]$CssY,
    [Parameter(Mandatory)][ValidateRange(100, 10000)][double]$ReferenceCssWidth
  )

  $scale = [double]$Bounds.width / $ReferenceCssWidth
  return [ordered]@{
    x = [int][Math]::Round(
      [double]$Bounds.x + ($CssX * $scale),
      [MidpointRounding]::AwayFromZero
    )
    y = [int][Math]::Round(
      [double]$Bounds.y + ($CssY * $scale),
      [MidpointRounding]::AwayFromZero
    )
    scale = $scale
    viewportCssHeight = [double]$Bounds.height / $scale
  }
}

function Invoke-FootprintClick {
  param(
    [Parameter(Mandatory)][IntPtr]$WindowHandle,
    [Parameter(Mandatory)][Collections.IDictionary]$Bounds,
    [Parameter(Mandatory)][double]$CssX,
    [Parameter(Mandatory)][double]$CssY,
    [double]$ReferenceCssWidth = 1380,
    [switch]$AssumeForeground
  )

  $point = ConvertTo-FootprintScreenPoint `
    -Bounds $Bounds `
    -CssX $CssX `
    -CssY $CssY `
    -ReferenceCssWidth $ReferenceCssWidth
  if ($AssumeForeground) {
    [SageFootprint.NativeMethods]::ClickVisible(
      $WindowHandle,
      [int]$point.x,
      [int]$point.y
    )
  } else {
    [SageFootprint.NativeMethods]::FocusAndClick(
      $WindowHandle,
      [int]$point.x,
      [int]$point.y
    )
  }
  return $point
}

function Get-FootprintRegionHash {
  param(
    [Parameter(Mandatory)][Collections.IDictionary]$Bounds,
    [Parameter(Mandatory)][double]$CssX,
    [Parameter(Mandatory)][double]$CssY,
    [Parameter(Mandatory)][double]$CssWidth,
    [Parameter(Mandatory)][double]$CssHeight,
    [double]$ReferenceCssWidth = 1380
  )

  $origin = ConvertTo-FootprintScreenPoint `
    -Bounds $Bounds `
    -CssX $CssX `
    -CssY $CssY `
    -ReferenceCssWidth $ReferenceCssWidth
  $width = [Math]::Max(1, [int][Math]::Round($CssWidth * $origin.scale))
  $height = [Math]::Max(1, [int][Math]::Round($CssHeight * $origin.scale))
  return [SageFootprint.NativeMethods]::CaptureHash(
    [int]$origin.x,
    [int]$origin.y,
    $width,
    $height
  )
}

function Wait-FootprintPaint {
  param(
    [Parameter(Mandatory)][Collections.IDictionary]$Bounds,
    [Parameter(Mandatory)][string]$BeforeHash,
    [Parameter(Mandatory)][double]$CssX,
    [Parameter(Mandatory)][double]$CssY,
    [Parameter(Mandatory)][double]$CssWidth,
    [Parameter(Mandatory)][double]$CssHeight,
    [ValidateRange(1, 5000)][int]$TimeoutMs = 1000
  )

  $timer = [Diagnostics.Stopwatch]::StartNew()
  while ($timer.ElapsedMilliseconds -lt $TimeoutMs) {
    $current = Get-FootprintRegionHash `
      -Bounds $Bounds `
      -CssX $CssX `
      -CssY $CssY `
      -CssWidth $CssWidth `
      -CssHeight $CssHeight
    if ($current -ne $BeforeHash) {
      return [Math]::Round($timer.Elapsed.TotalMilliseconds, 3)
    }
    Start-Sleep -Milliseconds 1
  }

  throw "No paint change was detected within $TimeoutMs ms."
}

function Invoke-FootprintScenario {
  param(
    [Parameter(Mandatory)][ValidateSet(
      "control",
      "control-static",
      "control-geometry",
      "control-scroll",
      "control-layered",
      "shell",
      "chat",
      "file",
      "diff",
      "explorer",
      "source-control"
    )][string]$Scenario,
    [Parameter(Mandatory)][IntPtr]$WindowHandle,
    [Parameter(Mandatory)][Collections.IDictionary]$Bounds
  )

  if ($Scenario -like "control*") {
    $controlX = switch ($Scenario) {
      "control-geometry" { 200 }
      "control-scroll" { 328 }
      "control-layered" { 456 }
      default { 72 }
    }
    $action = Invoke-FootprintClick $WindowHandle $Bounds $controlX 22
    Start-Sleep -Milliseconds 1200
    return [ordered]@{
      scenario = $Scenario
      actions = @($action)
      settleMs = 1200
    }
  }

  # Coordinates are CSS pixels in the fixed 1380-wide acceptance window.
  # Clicks land on the left side of tab activation buttons, so title width and
  # close-button visibility do not affect the target.
  $actions = @()
  $tabY = 63
  # The project chat entry is a more stable activation target than the first
  # tab: the tab strip can scroll when restored projects contain more tabs.
  $chatNavigationX = 72
  $chatNavigationY = 224
  $fileX = 484
  $terminalX = 602
  $railX = 1357
  $explorerRailY = 66
  $sourceControlRailY = 146

  switch ($Scenario) {
    "shell" {
      $actions += Invoke-FootprintClick $WindowHandle $Bounds $terminalX $tabY
    }
    "chat" {
      $actions += Invoke-FootprintClick `
        $WindowHandle `
        $Bounds `
        $chatNavigationX `
        $chatNavigationY
    }
    "file" {
      $actions += Invoke-FootprintClick $WindowHandle $Bounds $fileX $tabY
    }
    "explorer" {
      $actions += Invoke-FootprintClick `
        $WindowHandle `
        $Bounds `
        $chatNavigationX `
        $chatNavigationY
      Start-Sleep -Milliseconds 100
      $actions += Invoke-FootprintClick $WindowHandle $Bounds $railX $explorerRailY
    }
    "source-control" {
      $actions += Invoke-FootprintClick `
        $WindowHandle `
        $Bounds `
        $chatNavigationX `
        $chatNavigationY
      Start-Sleep -Milliseconds 100
      $actions += Invoke-FootprintClick $WindowHandle $Bounds $railX $sourceControlRailY
    }
    "diff" {
      $actions += Invoke-FootprintClick `
        $WindowHandle `
        $Bounds `
        $chatNavigationX `
        $chatNavigationY
      Start-Sleep -Milliseconds 100
      $actions += Invoke-FootprintClick $WindowHandle $Bounds $railX $sourceControlRailY
      Start-Sleep -Milliseconds 500
      # The first bounded Source Control change row follows the heading and
      # group header. The row opens a real structured Diff tab.
      $actions += Invoke-FootprintClick $WindowHandle $Bounds 1200 137
    }
  }

  Start-Sleep -Milliseconds 1200
  return [ordered]@{
    scenario = $Scenario
    actions = $actions
    settleMs = 1200
  }
}

function Invoke-FootprintInteractions {
  param(
    [Parameter(Mandatory)][IntPtr]$WindowHandle,
    [Parameter(Mandatory)][Collections.IDictionary]$Bounds,
    [ValidateRange(1, 100)][int]$Iterations = 20
  )

  [void](Invoke-FootprintScenario "chat" $WindowHandle $Bounds)
  # Keep the measured window above desktop notifications and the orchestrator
  # while physical input is injected. The process is closed after each launch.
  [SageFootprint.NativeMethods]::PinToFront($WindowHandle)
  $inputLatencies = @()

  for ($index = 0; $index -lt $Iterations; $index += 1) {
    [void](
      Invoke-FootprintClick `
        $WindowHandle `
        $Bounds `
        620 `
        690 `
        -AssumeForeground
    )
    Start-Sleep -Milliseconds 15
    $before = Get-FootprintRegionHash $Bounds 420 650 400 90
    $timer = [Diagnostics.Stopwatch]::StartNew()
    [SageFootprint.NativeMethods]::SendUnicode([char]0x78)
    $paint = Wait-FootprintPaint $Bounds $before 420 650 400 90
    $inputLatencies += [Math]::Round($timer.Elapsed.TotalMilliseconds, 3)
    [SageFootprint.NativeMethods]::PressVirtualKey(0x08)
    Start-Sleep -Milliseconds ([Math]::Max(10, [int]$paint))
  }

  $tabLatencies = @()
  for ($index = 0; $index -lt $Iterations; $index += 1) {
    $activateFile = $index % 2 -eq 0
    $targetX = if ($activateFile) { 484 } else { 72 }
    $targetY = if ($activateFile) { 63 } else { 224 }
    [SageFootprint.NativeMethods]::PinToFront($WindowHandle)
    $before = Get-FootprintRegionHash $Bounds 380 135 520 220
    $timer = [Diagnostics.Stopwatch]::StartNew()
    [void](
      Invoke-FootprintClick `
        $WindowHandle `
        $Bounds `
        $targetX `
        $targetY `
        -AssumeForeground
    )
    [void](Wait-FootprintPaint $Bounds $before 380 135 520 220)
    $tabLatencies += [Math]::Round($timer.Elapsed.TotalMilliseconds, 3)
    Start-Sleep -Milliseconds 40
  }

  return [ordered]@{
    iterations = $Iterations
    inputToPaintMs = $inputLatencies
    tabSwitchMs = $tabLatencies
  }
}

function Save-FootprintScreenshot {
  param(
    [Parameter(Mandatory)][Collections.IDictionary]$Bounds,
    [Parameter(Mandatory)][string]$Path
  )

  $directory = Split-Path -Parent $Path
  if (-not (Test-Path -LiteralPath $directory)) {
    [void](New-Item -ItemType Directory -Force -Path $directory)
  }
  [SageFootprint.NativeMethods]::CapturePng(
    [int]$Bounds.x,
    [int]$Bounds.y,
    [int]$Bounds.width,
    [int]$Bounds.height,
    $Path
  )
}

function Stop-FootprintTree {
  param(
    [Parameter(Mandatory)][Collections.IDictionary]$RootIdentity,
    [Parameter(Mandatory)][Collections.Generic.HashSet[int]]$KnownProcessIds,
    [ValidateRange(1, 30)][int]$GraceSeconds = 1
  )

  $rootProcessId = [int]$RootIdentity.processId
  $initialRows = @(Get-CimInstance Win32_Process)
  $initialMembers = @(
    Get-FootprintForceKillCandidates `
      -RootIdentity $RootIdentity `
      -ProcessRows $initialRows
  )
  $initialRoot = @(
    $initialMembers |
      Where-Object { [int]$_.ProcessId -eq $rootProcessId }
  ) | Select-Object -First 1
  if ($null -eq $initialRoot) {
    return [ordered]@{
      forced = $false
      identityLost = $true
      candidates = @()
      remaining = @()
    }
  }
  $root = Get-Process -Id $rootProcessId -ErrorAction SilentlyContinue
  if ($null -ne $root -and $root.MainWindowHandle -ne [IntPtr]::Zero) {
    [void]$root.CloseMainWindow()
  }

  $deadline = [DateTime]::UtcNow.AddSeconds($GraceSeconds)
  do {
    $rows = @(Get-CimInstance Win32_Process)
    $members = @(
      Get-FootprintForceKillCandidates `
        -RootIdentity $RootIdentity `
        -ProcessRows $rows
    )
    foreach ($row in $members) {
      [void]$KnownProcessIds.Add([int]$row.ProcessId)
    }
    if ($members.Count -eq 0) {
      return [ordered]@{
        forced = $false
        identityLost = $false
        candidates = @()
        remaining = @()
      }
    }
    Start-Sleep -Milliseconds 100
  } while ([DateTime]::UtcNow -lt $deadline)

  $candidateRows = @(
    Get-FootprintForceKillCandidates `
      -RootIdentity $RootIdentity `
      -ProcessRows @(Get-CimInstance Win32_Process)
  )
  $orderedCandidates = @(
    $candidateRows |
      Sort-Object `
        @{ Expression = { [int]$_.ProcessId -eq $rootProcessId } },
        @{ Expression = { [int]$_.ProcessId }; Descending = $true }
  )
  $forceCandidateIds = @($orderedCandidates | ForEach-Object { [int]$_.ProcessId })
  $killedIdentities = @()
  foreach ($candidate in $orderedCandidates) {
    # Re-query before every force kill. The root must still have the captured
    # executable/creation identity, the candidate must still be in its current
    # ancestry, and the candidate PID must still identify the same process row.
    $currentMembers = @(
      Get-FootprintForceKillCandidates `
        -RootIdentity $RootIdentity `
        -ProcessRows @(Get-CimInstance Win32_Process)
    )
    $current = @(
      $currentMembers |
        Where-Object { [int]$_.ProcessId -eq [int]$candidate.ProcessId }
    ) | Select-Object -First 1
    if (
      $null -eq $current -or
      -not $candidate.ExecutablePath -or
      -not $candidate.CreationDate -or
      -not (
        Test-FootprintProcessIdentity `
          -ProcessRow $current `
          -ExpectedExecutablePath ([string]$candidate.ExecutablePath) `
          -ExpectedCreationDate (
            ConvertTo-FootprintCreationIdentity $candidate.CreationDate
          )
      )
    ) {
      continue
    }
    Stop-Process `
      -Id ([int]$current.ProcessId) `
      -Force `
      -ErrorAction SilentlyContinue
    $killedIdentities += [ordered]@{
      processId = [int]$current.ProcessId
      executablePath = [string]$current.ExecutablePath
      creationDate = ConvertTo-FootprintCreationIdentity $current.CreationDate
    }
  }
  Start-Sleep -Milliseconds 250
  $finalRows = @(Get-CimInstance Win32_Process)
  $stillAlive = @(
    $killedIdentities |
      Where-Object {
        $identity = $_
        $current = @(
          $finalRows |
            Where-Object { [int]$_.ProcessId -eq [int]$identity.processId }
        ) | Select-Object -First 1
        $null -ne $current -and (
          Test-FootprintProcessIdentity `
            -ProcessRow $current `
            -ExpectedExecutablePath ([string]$identity.executablePath) `
            -ExpectedCreationDate ([string]$identity.creationDate)
        )
      } |
      ForEach-Object { [int]$_.processId }
  )
  return [ordered]@{
    forced = $killedIdentities.Count -gt 0
    identityLost = $false
    candidates = $forceCandidateIds
    remaining = $stillAlive
  }
}

Export-ModuleMember -Function @(
  "ConvertTo-FootprintScreenPoint",
  "Get-FootprintForceKillCandidates",
  "Get-FootprintDescendants",
  "Get-FootprintMedian",
  "Get-FootprintPercentile",
  "Get-FootprintProcessRole",
  "Get-FootprintRootIdentity",
  "Get-FootprintTreeSample",
  "Get-FootprintWebBounds",
  "Invoke-FootprintInteractions",
  "Invoke-FootprintScenario",
  "Save-FootprintScreenshot",
  "Stop-FootprintTree",
  "Wait-FootprintWindow"
)

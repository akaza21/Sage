param(
    [Parameter(Mandatory = $true)][int]$ProcessId,
    [Parameter(Mandatory = $true)][ValidateSet(1888, 720, 480)][int]$Width,
    [Parameter(Mandatory = $true)][int]$Port
)
$ErrorActionPreference = 'Stop'
$owner = Get-Process -Id $ProcessId
$owner.Refresh()
if ($owner.MainWindowHandle -eq 0) { throw 'native-window-missing' }
$listener = Get-NetTCPConnection -LocalPort $Port -State Listen | Select-Object -First 1
$ancestor = [int]$listener.OwningProcess
$owned = $false
for ($depth = 0; $depth -lt 8 -and $ancestor -gt 0; $depth++) {
    if ($ancestor -eq $ProcessId) { $owned = $true; break }
    $ancestor = [int](Get-CimInstance Win32_Process -Filter "ProcessId = $ancestor").ParentProcessId
}
if (!$owned) { throw 'cdp-owner-mismatch' }
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class NativeChatCaptureWindow {
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left; public int Top; public int Right; public int Bottom; }
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr window, int command);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr window, IntPtr after, int x, int y, int width, int height, uint flags);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out Rect rectangle);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr window, out Rect rectangle);
}
'@
$height = if ($Width -eq 1888) { 1096 } else { 1000 }
[void][NativeChatCaptureWindow]::ShowWindow($owner.MainWindowHandle, 9)
$outer = New-Object NativeChatCaptureWindow+Rect
$client = New-Object NativeChatCaptureWindow+Rect
$outerWidth = $Width
$outerHeight = $height
for ($attempt = 0; $attempt -lt 4; $attempt++) {
    if (![NativeChatCaptureWindow]::SetWindowPos($owner.MainWindowHandle, [IntPtr]::Zero, 0, 0, $outerWidth, $outerHeight, 0x0044)) { throw 'native-resize-failed' }
    if (![NativeChatCaptureWindow]::GetWindowRect($owner.MainWindowHandle, [ref]$outer) -or ![NativeChatCaptureWindow]::GetClientRect($owner.MainWindowHandle, [ref]$client)) { throw 'native-bounds-failed' }
    $clientWidth = $client.Right - $client.Left
    $clientHeight = $client.Bottom - $client.Top
    if ($clientWidth -eq $Width -and $clientHeight -eq $height) { break }
    $outerWidth = $outer.Right - $outer.Left + $Width - $clientWidth
    $outerHeight = $outer.Bottom - $outer.Top + $height - $clientHeight
}
$version = (Get-Process -Id $listener.OwningProcess).MainModule.FileVersionInfo.FileVersion
if ($version -notmatch '\d+(\.\d+){1,3}') { throw 'webview-version-missing' }
@{ outer = @{ width = $outer.Right - $outer.Left; height = $outer.Bottom - $outer.Top }; client = @{ width = $client.Right - $client.Left; height = $client.Bottom - $client.Top }; webviewVersion = $Matches[0] } | ConvertTo-Json -Compress

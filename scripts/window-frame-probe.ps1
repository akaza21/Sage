param([int]$ProcessId, [string]$Action = 'state', [int]$Cycles = 100, [string]$OutPng, [int]$X, [int]$Y, [string]$ControlName, [int]$Port)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing,System.Windows.Forms
Add-Type -ReferencedAssemblies System.Drawing @'
using System; using System.Runtime.InteropServices; using System.Text; using System.Collections.Generic;
public static class Probe {
 public delegate bool EnumProc(IntPtr h, IntPtr l);
 [StructLayout(LayoutKind.Sequential)] public struct Rect { public int L,T,R,B; }
 [StructLayout(LayoutKind.Sequential)] public struct Point { public int X,Y; }
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
 [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint command);
 [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint x, uint y, uint data, UIntPtr extra);
 [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc c, IntPtr l);
 [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr h, EnumProc c, IntPtr l);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
 [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr h);
 [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out Rect r);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out Rect r);
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
 [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x,int y,int w,int hh,uint f);
 [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern uint GetDoubleClickTime();
 [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(Point p);
 [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h,uint flags);
 [DllImport("user32.dll")] public static extern void keybd_event(byte vk,byte scan,uint flags,UIntPtr extra);
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint f);
 [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref Point p);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder b,int n);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder b,int n);
 [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h,int n);
 [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h,int a,out int v,int s);
 [DllImport("dwmapi.dll",EntryPoint="DwmGetWindowAttribute")] public static extern int DwmFrameBounds(IntPtr h,int a,out Rect v,int s);
 public static IntPtr Find(uint pid) { IntPtr found=IntPtr.Zero;long area=-1; EnumWindows((h,l)=>{uint p;GetWindowThreadProcessId(h,out p); Rect r; if(p==pid && GetWindow(h,4)==IntPtr.Zero && IsWindowVisible(h) && GetWindowRect(h,out r) && (long)(r.R-r.L)*(r.B-r.T)>area){area=(long)(r.R-r.L)*(r.B-r.T);found=h;}return true;},IntPtr.Zero);return found; }
 public static string Class(IntPtr h) { var b=new StringBuilder(256);GetClassName(h,b,256);return b.ToString(); }
 public static string Name(IntPtr h) { var b=new StringBuilder(256);GetWindowText(h,b,256);return b.ToString(); }
 public static List<IntPtr> Kids(IntPtr h) { var k=new List<IntPtr>();EnumChildWindows(h,(c,l)=>{k.Add(c);return true;},IntPtr.Zero);return k; }
}
'@
[void][Probe]::SetThreadDpiAwarenessContext([IntPtr](-4))
$h = [Probe]::Find($ProcessId)
if ($h -eq [IntPtr]::Zero) { throw 'No owned Sage window' }
$work = [System.Windows.Forms.Screen]::PrimaryScreen.WorkingArea
function Set-Size([int]$w,[int]$hh) {
 $wr=New-Object Probe+Rect; $cr=New-Object Probe+Rect
 [void][Probe]::GetWindowRect($h,[ref]$wr); [void][Probe]::GetClientRect($h,[ref]$cr)
 [void][Probe]::SetWindowPos($h,[IntPtr]::Zero,60,60,$w+$wr.R-$wr.L-$cr.R,$hh+$wr.B-$wr.T-$cr.B,0x14)
}
function Get-State {
 $wr=New-Object Probe+Rect; $cr=New-Object Probe+Rect
 [void][Probe]::GetWindowRect($h,[ref]$wr); [void][Probe]::GetClientRect($h,[ref]$cr)
 $kids=@(foreach($k in [Probe]::Kids($h)) { $r=New-Object Probe+Rect; [void][Probe]::GetWindowRect($k,[ref]$r); @{hwnd=[long]$k;cls=[Probe]::Class($k);name=[Probe]::Name($k);w=$r.R-$r.L;h=$r.B-$r.T;x=$r.L;y=$r.T;visible=[Probe]::IsWindowVisible($k)} })
 $p=New-Object Probe+Point;[void][Probe]::ClientToScreen($h,[ref]$p)
 $frame=New-Object Probe+Rect;[void][Probe]::DwmFrameBounds($h,9,[ref]$frame,16)
 @{hwnd=[long]$h;cls=[Probe]::Class($h);name=[Probe]::Name($h);iconic=[Probe]::IsIconic($h);zoomed=[Probe]::IsZoomed($h);x=$wr.L;y=$wr.T;frameT=$frame.T;frameL=$frame.L;frameW=$frame.R-$frame.L;frameH=$frame.B-$frame.T;clientX=$p.X;clientY=$p.Y;w=$wr.R-$wr.L;h=$wr.B-$wr.T;clientW=$cr.R;clientH=$cr.B;dpi=[Probe]::GetDpiForWindow($h);children=$kids;workArea=@{x=$work.X;y=$work.Y;w=$work.Width;h=$work.Height}}
}
function Initialize-NativeAutomation {
 Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,WindowsBase,UIAutomationClientsideProviders
 # Initialize the managed client's proxy manager before registering Win32 providers.
 $null=[System.Windows.Automation.AutomationElement]::RootElement
 [System.Windows.Automation.ClientSettings]::RegisterClientSideProviders([UIAutomationClientsideProviders.UIAutomationClientSideProviders]::ClientSideProviderDescriptionTable)
}
function Set-OwnedForeground {
 if([Probe]::GetForegroundWindow() -ne $h){
  Initialize-NativeAutomation
  [System.Windows.Automation.AutomationElement]::FromHandle($h).SetFocus()
  Start-Sleep -Milliseconds 200
 }
 $foreground=[Probe]::GetForegroundWindow()
 if($foreground -ne $h){throw ("Sage did not acquire foreground; refusing input (foreground class: " + [Probe]::Class($foreground) + ")")}
}
function Set-OwnedPointer([int]$px,[int]$py) {
 Set-OwnedForeground
 $point=New-Object Probe+Point;$point.X=$px;$point.Y=$py
 if([Probe]::GetAncestor([Probe]::WindowFromPoint($point),2) -ne $h){throw 'Pointer target is not Sage; refusing input'}
 [void][Probe]::SetCursorPos($px,$py)
}
switch($Action) {
 'owner' {
  $listener=Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction Stop | Select-Object -First 1
  $ancestor=[int]$listener.OwningProcess
  for($i=0;$i -lt 12 -and $ancestor -ne $ProcessId -and $ancestor -gt 0;$i++){
   $ancestor=[int](Get-CimInstance Win32_Process -Filter "ProcessId=$ancestor").ParentProcessId
  }
  if($ancestor -ne $ProcessId){throw 'Debug endpoint does not belong to this Sage process'}
 }
 'serve' {
  Get-State | ConvertTo-Json -Depth 5 -Compress
  while($null -ne ($line=[Console]::ReadLine())){
   switch($line){
    'sliver' {[void][Probe]::ShowWindow($h,9);Set-Size 661 87}
    'min' {[void][Probe]::ShowWindow($h,7)}
    'restore-only' {[void][Probe]::ShowWindow($h,4);Set-Size 1600 900}
    'restore' {[void][Probe]::ShowWindow($h,7);Start-Sleep -Milliseconds 10;[void][Probe]::ShowWindow($h,4);Set-Size 1600 900}
    'state' {}
    'quit' {[void][Probe]::ShowWindow($h,7);exit 0}
    default {throw 'Unknown native probe command'}
   }
   Get-State | ConvertTo-Json -Depth 5 -Compress
  }
  exit 0
 }
 'doubleclick' {
  Set-OwnedPointer $X $Y
  $dwell=[Math]::Max(10,[Math]::Min(75,[int]([Probe]::GetDoubleClickTime()/4)))
  try{for($i=0;$i -lt 2;$i++){[Probe]::mouse_event(2,0,0,0,[UIntPtr]::Zero);Start-Sleep -Milliseconds $dwell;[Probe]::mouse_event(4,0,0,0,[UIntPtr]::Zero);Start-Sleep -Milliseconds $dwell}}
  finally{[Probe]::mouse_event(4,0,0,0,[UIntPtr]::Zero)}
 }
 'cancelclick' {
  Set-OwnedPointer $X $Y
  try{[Probe]::mouse_event(2,0,0,0,[UIntPtr]::Zero);Start-Sleep -Milliseconds 100;[void][Probe]::SetCursorPos(($X-100),$Y);Start-Sleep -Milliseconds 100}
  finally{[Probe]::mouse_event(4,0,0,0,[UIntPtr]::Zero)}
 }
 'returnclick' {
  Set-OwnedPointer $X $Y
  try{[Probe]::mouse_event(2,0,0,0,[UIntPtr]::Zero);Start-Sleep -Milliseconds 100;[void][Probe]::SetCursorPos(($X-100),$Y);Start-Sleep -Milliseconds 100;[void][Probe]::SetCursorPos($X,$Y);Start-Sleep -Milliseconds 100}
  finally{[Probe]::mouse_event(4,0,0,0,[UIntPtr]::Zero)}
 }
 'drag' {
  Set-OwnedPointer $X $Y
  try{[Probe]::mouse_event(2,0,0,0,[UIntPtr]::Zero);Start-Sleep -Milliseconds 100;[void][Probe]::SetCursorPos(($X+80),($Y+40));Start-Sleep -Milliseconds 200}
  finally{[Probe]::mouse_event(4,0,0,0,[UIntPtr]::Zero)}
 }
 'borders' {
  $state=Get-State
  $left=$state.x+1;$right=$state.x+$state.w-2;$top=$state.y+1;$bottom=$state.y+$state.h-2
  $midX=$state.x+[int]($state.w/2);$midY=$state.y+[int]($state.h/2)
  $points=@(@($left,$midY,10),@($right,$midY,11),@($midX,$top,12),@($midX,$bottom,15),@($left,$top,13),@($right,$top,14),@($left,$bottom,16),@($right,$bottom,17))
  $hits=@(foreach($point in $points){
   $actual=[Probe]::SendMessage($h,0x84,[IntPtr]::Zero,[IntPtr](($point[1] -shl 16) -bor ($point[0] -band 65535)))
   @{expected=$point[2];actual=[long]$actual;x=$point[0];y=$point[1]}
  })
  @{hits=$hits} | ConvertTo-Json -Depth 3 -Compress
  exit 0
 }
 'hit' {
  Set-OwnedForeground
  $p=New-Object Probe+Point;$p.X=$X;$p.Y=$Y
  $target=[Probe]::WindowFromPoint($p)
  if([Probe]::GetAncestor($target,2) -ne $h){throw 'Hit-test target is not Sage'}
  $hit=[Probe]::SendMessage($target,0x84,[IntPtr]::Zero,[IntPtr](($Y -shl 16) -bor ($X -band 65535)))
  @{hit=[long]$hit} | ConvertTo-Json -Compress
  exit 0
 }
 'restore' { [void][Probe]::ShowWindow($h,9); Set-Size 1600 900 }
 'max' { [void][Probe]::ShowWindow($h,3) }
 'min' { [void][Probe]::ShowWindow($h,7) }
 'half' { [void][Probe]::ShowWindow($h,9); [void][Probe]::SetWindowPos($h,[IntPtr]::Zero,$work.X,$work.Y,([int]($work.Width/2)),$work.Height,0x14) }
 'snap' {
  [void][Probe]::ShowWindow($h,9);Set-Size 1600 900;Set-OwnedForeground
  try{[Probe]::keybd_event(0x5B,0,0,[UIntPtr]::Zero);[Probe]::keybd_event(0x25,0,0,[UIntPtr]::Zero)}
  finally{[Probe]::keybd_event(0x25,0,2,[UIntPtr]::Zero);[Probe]::keybd_event(0x5B,0,2,[UIntPtr]::Zero)}
 }
 'hover' {
  Set-OwnedPointer $X $Y
  Start-Sleep -Milliseconds 1200

 }
 'uia' {
  Initialize-NativeAutomation
  $toInvoke=$null
  $nativeButtons=@(foreach($kid in (Get-State).children | Where-Object { $_.cls -eq 'Button' -and $_.name -match '^(Minimize|Maximize|Restore|Close) window$' }) {
   $element=[System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$kid.hwnd)
   $pattern=$null
   $available=$element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)
   if($ControlName -and $element.Current.Name -eq $ControlName){$toInvoke=$pattern}
   @{name=$element.Current.Name;role=$element.Current.ControlType.ProgrammaticName;invoke=$available;enabled=$element.Current.IsEnabled;offscreen=$element.Current.IsOffscreen;hwnd=$kid.hwnd}
  })
  if($ControlName -and !$toInvoke){throw "Native caption control unavailable: $ControlName"}
  if($toInvoke){$toInvoke.Invoke()}
  @{buttons=$nativeButtons} | ConvertTo-Json -Depth 3 -Compress
 }
 'stress' {
  $failures=@(); $checked=0; $transient=0
  for($i=0;$i -lt $Cycles;$i++) {
   [void][Probe]::ShowWindow($h,9); Set-Size (800+($i%4)*200) (600+($i%3)*100)
   if($i%3 -eq 0){[void][Probe]::ShowWindow($h,3)}
   [void][Probe]::ShowWindow($h,(@(2,6,7)[$i%3]))
   if($i%4 -eq 0){[void][Probe]::SetWindowPos($h,[IntPtr]::Zero,60,60,677,126,0x14)}
   if($i%5 -eq 0){[void][Probe]::ShowWindow($h,(@(6,7)[$i%2]))}
   [void][Probe]::ShowWindow($h,(@(4,9)[$i%2])); Set-Size 1600 900
   Start-Sleep -Milliseconds 100
   $state=Get-State
   $render=@($state.children | Where-Object { $_.cls -eq 'Chrome_RenderWidgetHostHWND' -and $_.visible }) | Select-Object -First 1
   if($state.iconic -or !$render){throw 'Restored Sage has no visible renderer'}
   if(!$state.iconic -and $render) {
    $checked++
    if($render.w -ne $state.clientW -or $render.h -ne $state.clientH){
     Start-Sleep -Seconds 5;$settled=Get-State
     $finalRender=@($settled.children | Where-Object { $_.cls -eq 'Chrome_RenderWidgetHostHWND' -and $_.visible }) | Select-Object -First 1
     if($finalRender.w -ne $settled.clientW -or $finalRender.h -ne $settled.clientH){$failures+=@{cycle=$i;state=$state;settled=$settled};break}
     $transient++
    }
   }
  }
  @{checked=$checked;transient=$transient;failures=$failures} | ConvertTo-Json -Depth 7 -Compress
 }
}
if ($Action -ne 'stress' -and $Action -ne 'uia') {
 Start-Sleep -Milliseconds 650
 if($OutPng){
  $r=New-Object Probe+Rect;[void][Probe]::GetWindowRect($h,[ref]$r)
  $bmp=New-Object System.Drawing.Bitmap ($r.R-$r.L),($r.B-$r.T);$g=[System.Drawing.Graphics]::FromImage($bmp);$dc=$g.GetHdc()
  $ok=[Probe]::PrintWindow($h,$dc,2);$g.ReleaseHdc($dc);$g.Dispose();$bmp.Save($OutPng,[System.Drawing.Imaging.ImageFormat]::Png)
  $colors=@(foreach($kid in (Get-State).children | Where-Object { $_.cls -eq 'Button' -and $_.visible -and $_.name -match 'window$' }){
    $pixel=$bmp.GetPixel(($kid.x-$r.L+[int]($kid.w/4)),($kid.y-$r.T+[int]($kid.h/2)))
    @{name=$kid.name;r=$pixel.R;g=$pixel.G;b=$pixel.B}
  }); $bmp.Dispose()
  if(!$ok){throw 'PrintWindow failed'}
  $screen=New-Object System.Drawing.Bitmap ($r.R-$r.L),($r.B-$r.T)
  $graphics=[System.Drawing.Graphics]::FromImage($screen)
  $graphics.CopyFromScreen($r.L,$r.T,0,0,$screen.Size)
  $screen.Save(([IO.Path]::ChangeExtension($OutPng,'screen.png')),[System.Drawing.Imaging.ImageFormat]::Png)
  $graphics.Dispose();$screen.Dispose()
 }
 $state=Get-State;if($OutPng){$state.pixels=$colors};; $state | ConvertTo-Json -Depth 5 -Compress
}

# Temporary measurement (not for main): what the main window shows from the moment
# it appears until the interface draws, on Windows. Starts the launcher several
# times per theme and language, grabs the window's client area as often as it can
# and names each frame: none (no window yet), white, light-ground or dark-ground,
# silhouette (the rail without text) or interface (the rail with text). The
# details are white%, light ground%, dark ground% beside the rail, rail% and the
# bright pixels in the rail.
# Prints fixed words and numbers only; saves some frames as PNG (Windows PowerShell 5.1).
param(
  [Parameter(Mandatory)] [string] $Exe,
  [Parameter(Mandatory)] [string] $DataRoot,
  [Parameter(Mandatory)] [string] $Out,
  [int] $Starts = 2,
  [int] $Ms = 3000
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using System.Threading;

public static class FirstFrame {
  delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] static extern IntPtr GetWindow(IntPtr h, uint cmd);
  [DllImport("user32.dll")] static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [StructLayout(LayoutKind.Sequential)] struct RECT { public int L, T, R, B; }
  [StructLayout(LayoutKind.Sequential)] struct POINT { public int X, Y; }

  static IntPtr MainWindow(uint pid) {
    IntPtr found = IntPtr.Zero; int best = 0;
    EnumWindows((h, l) => {
      uint p; GetWindowThreadProcessId(h, out p);
      if (p == pid && IsWindowVisible(h) && GetWindow(h, 4) == IntPtr.Zero) {
        RECT r; GetClientRect(h, out r);
        int area = (r.R - r.L) * (r.B - r.T);
        if (area > best) { best = area; found = h; }
      }
      return true;
    }, IntPtr.Zero);
    return best >= 400 * 300 ? found : IntPtr.Zero;
  }

  public class Frame { public long T; public Bitmap Image; }

  public static int LastPid;

  // Starts exe and grabs the client area of its main window for ms milliseconds.
  public static List<Frame> Run(string exe, int ms) {
    var frames = new List<Frame>();
    var clock = Stopwatch.StartNew();
    var process = Process.Start(new ProcessStartInfo(exe) { UseShellExecute = false });
    LastPid = process.Id;
    uint pid = (uint)process.Id;
    while (clock.ElapsedMilliseconds < ms) {
      IntPtr h = MainWindow(pid);
      if (h == IntPtr.Zero) { frames.Add(new Frame { T = clock.ElapsedMilliseconds }); Thread.Sleep(5); continue; }
      RECT r; GetClientRect(h, out r);
      POINT o = new POINT(); ClientToScreen(h, ref o);
      int w = r.R - r.L, hh = r.B - r.T;
      if (w < 400 || hh < 300) { Thread.Sleep(5); continue; }
      var bmp = new Bitmap(w, hh, PixelFormat.Format32bppArgb);
      using (var g = Graphics.FromImage(bmp)) g.CopyFromScreen(o.X, o.Y, 0, 0, new Size(w, hh));
      frames.Add(new Frame { T = clock.ElapsedMilliseconds, Image = bmp });
    }
    return frames;
  }

  static readonly int[][] Rail = {
    new[] { 0x12, 0x3b, 0x2a }, new[] { 0x0b, 0x22, 0x19 }, // --color-rail
    new[] { 37, 75, 59 }, new[] { 26, 47, 39 },             // --color-rail-field over it
  };

  static bool Near(int r, int g, int b, int[] c, int d) {
    return Math.Abs(r - c[0]) <= d && Math.Abs(g - c[1]) <= d && Math.Abs(b - c[2]) <= d;
  }

  public static string Name(int r, int g, int b) {
    if (r >= 250 && g >= 250 && b >= 250) return "white";
    if (Near(r, g, b, new[] { 0xf3, 0xf5, 0xf0 }, 3)) return "light-ground";
    if (Near(r, g, b, new[] { 0x0f, 0x17, 0x13 }, 3)) return "dark-ground";
    foreach (var c in Rail) if (Near(r, g, b, c, 3)) return "rail";
    return "other";
  }

  // state|white%|light-ground%|dark-ground%|rail%|bright
  public static string Describe(Bitmap b, bool rtl) {
    int w = b.Width, h = b.Height;
    var data = b.LockBits(new Rectangle(0, 0, w, h), ImageLockMode.ReadOnly, PixelFormat.Format32bppArgb);
    var px = new byte[data.Stride * h];
    Marshal.Copy(data.Scan0, px, 0, px.Length);
    int stride = data.Stride;
    b.UnlockBits(data);
    // The rail is 15rem (240 px) from 1040 px up, else 4.5rem (72 px); the
    // bottom rows may lie under the taskbar on a small screen.
    int railW = w >= 1040 ? 240 : 72;
    int bottom = h - 120;
    int x0 = rtl ? w - railW : 0, x1 = rtl ? w : railW;
    int white = 0, light = 0, dark = 0, all = 0;
    int rail = 0, bright = 0, strip = 0;
    for (int y = 0; y < bottom; y += 2) for (int x = 0; x < w; x += 2) {
      int i = y * stride + x * 4;
      int r = px[i + 2], g = px[i + 1], bl = px[i];
      if (x >= x0 && x < x1) {
        strip++;
        foreach (var k in Rail) if (Near(r, g, bl, k, 3)) { rail++; break; }
        if (Math.Min(r, Math.Min(g, bl)) >= 130) bright++;
      } else {
        all++;
        if (r >= 250 && g >= 250 && bl >= 250) white++;
        else if (Near(r, g, bl, new[] { 0xf3, 0xf5, 0xf0 }, 3)) light++;
        else if (Near(r, g, bl, new[] { 0x0f, 0x17, 0x13 }, 3)) dark++;
      }
    }
    int whitePct = white * 100 / all, lightPct = light * 100 / all, darkPct = dark * 100 / all;
    int railPct = rail * 100 / strip;
    string state;
    if (whitePct >= 90 && railPct < 5) state = "white";
    else if (railPct >= 50 && bright >= 30) state = "interface";
    else if (railPct >= 50) state = "silhouette";
    else if (lightPct >= 90) state = "light-ground";
    else if (darkPct >= 90) state = "dark-ground";
    else state = "other";
    return state + "|" + whitePct + "|" + lightPct + "|" + darkPct + "|" + railPct + "|" + bright;
  }
}
'@
[FirstFrame]::SetProcessDPIAware() | Out-Null

$scenarios = @(
  @{ name = 'light'; theme = 'light'; language = 'en'; appsLight = 1 },
  @{ name = 'dark'; theme = 'dark'; language = 'en'; appsLight = 1 },
  @{ name = 'dark-arabic'; theme = 'dark'; language = 'ar'; appsLight = 1 },
  @{ name = 'system-dark'; theme = 'system'; language = 'en'; appsLight = 0 },
  @{ name = 'system-light'; theme = 'system'; language = 'en'; appsLight = 1 },
  @{ name = 'light-on-dark-system'; theme = 'light'; language = 'en'; appsLight = 0 }
)
$personalize = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize'
if (-not (Test-Path $personalize)) { New-Item -Path $personalize -Force | Out-Null }
$shell = New-Object -ComObject Shell.Application
New-Item -ItemType Directory -Force $Out, $DataRoot | Out-Null

function Stop-Launcher([int] $id) {
  $p = Get-Process -Id $id -ErrorAction SilentlyContinue
  $how = 'gone'
  if ($p) {
    $null = $p.CloseMainWindow()
    if (-not $p.WaitForExit(15000)) { Stop-Process -Id $id -Force; $how = 'forced' } else { $how = 'closed' }
  }
  Start-Sleep -Seconds 3
  return $how
}

# A first start makes WebView2's profile, which a player's later starts already have.
$warm = [FirstFrame]::Run($Exe, 6000)
foreach ($f in $warm) { if ($f.Image) { $f.Image.Dispose() } }
Write-Host "warm-up start: $(Stop-Launcher ([FirstFrame]::LastPid))"

foreach ($s in $scenarios) {
  Set-ItemProperty -Path $personalize -Name AppsUseLightTheme -Value $s.appsLight -Type DWord
  $settings = '{"schemaVersion":1,"theme":"' + $s.theme + '","language":"' + $s.language + '"}'
  [IO.File]::WriteAllText((Join-Path $DataRoot 'settings.json'), $settings)
  $rtl = $s.language -eq 'ar'
  for ($i = 1; $i -le $Starts; $i++) {
    # Nothing in front of the window as it opens (the runner's console is).
    $shell.MinimizeAll()
    Start-Sleep -Milliseconds 800
    $frames = [FirstFrame]::Run($Exe, $Ms)
    $closed = Stop-Launcher ([FirstFrame]::LastPid)
    $runs = @()
    $prev = $null
    $size = ''
    $nextSave = 0
    foreach ($f in $frames) {
      # The sequence of the last start, a frame every 100 ms up to 1.6 s.
      if ($i -eq $Starts -and $f.Image -and $f.T -ge $nextSave -and $f.T -le 1600) {
        $f.Image.Save((Join-Path $Out "$($s.name)-seq-$('{0:D4}' -f $f.T)ms.png"), [System.Drawing.Imaging.ImageFormat]::Png)
        $nextSave = $f.T + 100
      }
      if ($f.Image) {
        $d = [FirstFrame]::Describe($f.Image, $rtl)
        $size = "$($f.Image.Width)x$($f.Image.Height)"
      } else { $d = 'none|-|-|-|-|-' }
      $state = $d.Split('|')[0]
      if ($state -ne $prev) {
        $runs += [pscustomobject]@{ state = $state; from = $f.T; detail = $d }
        if ($i -eq $Starts -and $f.Image) {
          $f.Image.Save((Join-Path $Out "$($s.name)-$($f.T)ms-$state.png"), [System.Drawing.Imaging.ImageFormat]::Png)
        }
        $prev = $state
      }
    }
    $last = $frames[$frames.Count - 1]
    if ($i -eq $Starts -and $last.Image) { $last.Image.Save((Join-Path $Out "$($s.name)-$($last.T)ms-last.png"), [System.Drawing.Imaging.ImageFormat]::Png) }
    $line = for ($k = 0; $k -lt $runs.Count; $k++) {
      $to = if ($k + 1 -lt $runs.Count) { $runs[$k + 1].from } else { $last.T }
      "$($runs[$k].state) $($runs[$k].from)-$to"
    }
    Write-Host "$($s.name) start ${i} ($size, $($frames.Count) frames, $closed): $($line -join ' | ')"
    Write-Host "   first frame of each: $(($runs | ForEach-Object { "$($_.state)=[$($_.detail)]" }) -join ' ')"
    foreach ($f in $frames) { if ($f.Image) { $f.Image.Dispose() } }
  }
}
Set-ItemProperty -Path $personalize -Name AppsUseLightTheme -Value 1 -Type DWord

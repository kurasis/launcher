# Player's-eye check of game versions on the Windows test machine (docs/USERTEST.md).
# Usage: powershell -NoProfile -ExecutionPolicy Bypass -File windows.ps1 `
#          -Setup <setup.exe> -Strings <ui-strings.json> -Versions 26.1,26.2 -Out <folder>
#
# Installs the packaged launcher into its own folder in portable mode, then for each
# version does what a player does: Accounts -> offline profile, Installations -> New
# installation -> the version -> Create -> Install -> Play; in the game Singleplayer ->
# Create New World, a look around, and the window closed. The launcher is driven through
# UI Automation, the game through window messages (clicks and keys, as a mouse and a
# keyboard send them), so a locked screen does not stop it. Nothing is changed in the
# game files or settings. Results go to <Out>/results.json; F2 screenshots and the
# game's log (machine names replaced) to <Out>/<version>/.
#
# Plain ASCII only: Windows PowerShell reads this file in the ANSI code page. The
# interface texts come from ui-strings.json (every catalog's text for each key).
param(
  [Parameter(Mandatory = $true)][string]$Setup,
  [Parameter(Mandatory = $true)][string]$Strings,
  [Parameter(Mandatory = $true)][string]$Versions,
  [Parameter(Mandatory = $true)][string]$Out,
  [string]$Nick = 'Tester'
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Drawing

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
namespace UserTest {
  public static class Win {
    public delegate bool EnumProc(IntPtr hwnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int max);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr hwnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hwnd, uint msg, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr value);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, IntPtr extra);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }

    // Visible top-level windows whose title starts with the prefix: "hwnd|pid|title".
    public static List<string> Find(string prefix) {
      var found = new List<string>();
      EnumWindows((h, l) => {
        if (!IsWindowVisible(h)) return true;
        var sb = new StringBuilder(512);
        GetWindowText(h, sb, sb.Capacity);
        var title = sb.ToString();
        if (title.StartsWith(prefix, StringComparison.Ordinal)) {
          uint pid; GetWindowThreadProcessId(h, out pid);
          found.Add(h.ToInt64() + "|" + pid + "|" + title);
        }
        return true;
      }, IntPtr.Zero);
      return found;
    }
  }
}
'@
# Physical pixels everywhere: the game's window is DPI aware.
[UserTest.Win]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null

$AE = [System.Windows.Automation.AutomationElement]
$Scope = [System.Windows.Automation.TreeScope]
$Any = [System.Windows.Automation.Condition]::TrueCondition
$ui = [IO.File]::ReadAllText($Strings, [Text.Encoding]::UTF8) | ConvertFrom-Json
New-Item -ItemType Directory -Force $Out | Out-Null

function Say($text) { Write-Host "[$((Get-Date).ToString('HH:mm:ss'))] $text" }

function Wait-For([scriptblock]$Find, [int]$Seconds = 30) {
  $deadline = (Get-Date).AddSeconds($Seconds)
  do {
    $found = & $Find
    if ($found) { return $found }
    Start-Sleep -Milliseconds 500
  } while ((Get-Date) -lt $deadline)
  return $null
}

# --- The launcher, through UI Automation -------------------------------------------

function Find-Window([string]$Name) {
  $condition = New-Object System.Windows.Automation.PropertyCondition($AE::NameProperty, $Name)
  return $AE::RootElement.FindFirst($Scope::Children, $condition)
}

# Any of the key's texts (every language), compared without case: WebView2 reports
# CSS-transformed text. A pattern ending in '*' matches a name's start.
function Matches([string]$Name, $Texts) {
  foreach ($text in @($Texts)) {
    if ($text.EndsWith('*')) {
      if ($Name.StartsWith($text.TrimEnd('*'), [StringComparison]::OrdinalIgnoreCase)) { return $true }
    } elseif ($Name -ieq $text) { return $true }
  }
  return $false
}

# A focusable, enabled element named by one of the texts.
function Find-Pressable($Texts) {
  foreach ($candidate in $script:main.FindAll($Scope::Descendants, $Any)) {
    try {
      $c = $candidate.Current
      if ($c.IsKeyboardFocusable -and $c.IsEnabled -and (Matches $c.Name $Texts)) { return $candidate }
    } catch { }
  }
  return $null
}

function Find-AllPressable($Texts) {
  foreach ($candidate in $script:main.FindAll($Scope::Descendants, $Any)) {
    try {
      $c = $candidate.Current
      if ($c.IsKeyboardFocusable -and $c.IsEnabled -and (Matches $c.Name $Texts)) { $candidate }
    } catch { }
  }
}

function Find-Input($Texts) {
  $edit = [System.Windows.Automation.ControlType]::Edit
  $byType = New-Object System.Windows.Automation.PropertyCondition($AE::ControlTypeProperty, $edit)
  foreach ($candidate in $script:main.FindAll($Scope::Descendants, $byType)) {
    if (Matches $candidate.Current.Name $Texts) { return $candidate }
  }
  return $null
}

function Get-Texts {
  $all = $script:main.FindAll($Scope::Descendants, $Any)
  return @($all | ForEach-Object { try { $_.Current.Name } catch { } } | Where-Object { $_ })
}

function Click($Element) {
  $pattern = $null
  if ($Element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
    $pattern.Invoke()
    return
  }
  if ($Element.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern, [ref]$pattern)) {
    $pattern.Select()
    return
  }
  try { $script:main.SetFocus() } catch { }
  $r = $Element.Current.BoundingRectangle
  [UserTest.Win]::SetCursorPos([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2)) | Out-Null
  Start-Sleep -Milliseconds 100
  [UserTest.Win]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero)
  [UserTest.Win]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero)
}

function Press($Texts, [string]$What, [int]$Seconds = 30) {
  $element = Wait-For { Find-Pressable $Texts } $Seconds
  if (-not $element) { throw "not found in the launcher: $What" }
  Click $element
  Start-Sleep -Milliseconds 700
}

function Type-Into($Element, [string]$Text) {
  $pattern = $null
  if ($Element.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
    $pattern.SetValue($Text)
  }
  Start-Sleep -Milliseconds 300
  if ($pattern -and $pattern.Current.Value -eq $Text) { return }
  # Keyboard as a fallback (needs an unlocked screen).
  Add-Type -AssemblyName System.Windows.Forms
  $Element.SetFocus()
  [System.Windows.Forms.SendKeys]::SendWait($Text)
}

function Launcher-Screenshot([string]$Path) {
  try {
    $r = $script:main.Current.BoundingRectangle
    $bitmap = New-Object System.Drawing.Bitmap([int]$r.Width, [int]$r.Height)
    [System.Drawing.Graphics]::FromImage($bitmap).CopyFromScreen([int]$r.X, [int]$r.Y, 0, 0, $bitmap.Size)
    $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
  } catch { Say "no launcher screenshot: $($_.Exception.Message)" }
}

# What the launcher shows, for the record of a failure (no player or machine data:
# the nick is the test one, paths are replaced).
function Dump-Launcher([string]$Folder) {
  Get-Texts | Select-Object -First 200 | ForEach-Object { Scrub $_ } |
    Set-Content -Encoding UTF8 (Join-Path $Folder 'launcher-texts.txt')
  Launcher-Screenshot (Join-Path $Folder 'launcher.png')
}

# --- The game, through window messages ------------------------------------------------

$WM_MOUSEMOVE = 0x0200; $WM_LBUTTONDOWN = 0x0201; $WM_LBUTTONUP = 0x0202
$WM_KEYDOWN = 0x0100; $WM_KEYUP = 0x0101; $WM_CLOSE = 0x0010
$Keys = @{ Escape = @(0x1B, 0x01); Enter = @(0x0D, 0x1C); Tab = @(0x09, 0x0F); F2 = @(0x71, 0x3C) }

function Game-Key([IntPtr]$Hwnd, [string]$Key) {
  $vk, $sc = $Keys[$Key]
  [UserTest.Win]::PostMessage($Hwnd, $WM_KEYDOWN, [IntPtr]$vk, [IntPtr][int64](1 -bor ($sc -shl 16))) | Out-Null
  Start-Sleep -Milliseconds 120
  $up = [int64]1 -bor ([int64]$sc -shl 16) -bor ([int64]1 -shl 30) -bor ([int64]1 -shl 31)
  [UserTest.Win]::PostMessage($Hwnd, $WM_KEYUP, [IntPtr]$vk, [IntPtr]$up) | Out-Null
  Start-Sleep -Milliseconds 400
}

# The game's GUI scale on "auto", as Minecraft computes it from the window size.
function Gui-Layout([IntPtr]$Hwnd) {
  $rect = New-Object UserTest.Win+RECT
  [UserTest.Win]::GetClientRect($Hwnd, [ref]$rect) | Out-Null
  $w = $rect.Right - $rect.Left; $h = $rect.Bottom - $rect.Top
  $scale = 1
  while ($scale -lt $w -and $scale -lt $h -and [math]::Floor($w / ($scale + 1)) -ge 320 -and [math]::Floor($h / ($scale + 1)) -ge 240) { $scale++ }
  return [pscustomobject]@{ Width = $w; Height = $h; Scale = $scale
    GuiWidth = [math]::Ceiling($w / $scale); GuiHeight = [math]::Ceiling($h / $scale) }
}

# A click at GUI coordinates.
function Game-Click([IntPtr]$Hwnd, [double]$GuiX, [double]$GuiY) {
  $layout = Gui-Layout $Hwnd
  $x = [int]($GuiX * $layout.Scale); $y = [int]($GuiY * $layout.Scale)
  $lp = [IntPtr][int64](($y -shl 16) -bor ($x -band 0xFFFF))
  [UserTest.Win]::SetForegroundWindow($Hwnd) | Out-Null
  [UserTest.Win]::PostMessage($Hwnd, $WM_MOUSEMOVE, [IntPtr]::Zero, $lp) | Out-Null
  Start-Sleep -Milliseconds 200
  [UserTest.Win]::PostMessage($Hwnd, $WM_MOUSEMOVE, [IntPtr]::Zero, $lp) | Out-Null
  Start-Sleep -Milliseconds 200
  [UserTest.Win]::PostMessage($Hwnd, $WM_LBUTTONDOWN, [IntPtr]1, $lp) | Out-Null
  Start-Sleep -Milliseconds 100
  [UserTest.Win]::PostMessage($Hwnd, $WM_LBUTTONUP, [IntPtr]::Zero, $lp) | Out-Null
  Start-Sleep -Milliseconds 500
}

function Game-Window {
  foreach ($entry in [UserTest.Win]::Find('Minecraft')) {
    $hwnd, $id, $title = $entry -split '\|', 3
    $process = Get-Process -Id ([int]$id) -ErrorAction SilentlyContinue
    if ($process -and $process.ProcessName -like 'java*') {
      return [pscustomobject]@{ Hwnd = [IntPtr][int64]$hwnd; Process = $process; Title = $title }
    }
  }
  return $null
}

function Log-Text([string]$Log) {
  if (-not (Test-Path $Log)) { return '' }
  try {
    $stream = [IO.File]::Open($Log, 'Open', 'Read', 'ReadWrite,Delete')
    try { return (New-Object IO.StreamReader($stream)).ReadToEnd() } finally { $stream.Dispose() }
  } catch { return '' }
}

# --- Machine names out of anything uploaded (the repository is public) -------------

$machine = @($env:USERPROFILE, $env:LOCALAPPDATA, $env:APPDATA, $env:USERNAME, $env:COMPUTERNAME) |
  Where-Object { $_ -and $_.Length -ge 3 } | Sort-Object Length -Descending
function Scrub([string]$Text) {
  foreach ($name in $machine) { $Text = $Text.Replace($name, '<machine>') }
  return $Text
}

# --- Install the launcher ---------------------------------------------------------

$root = Join-Path $env:LOCALAPPDATA 'launcher-usertest'
$appDir = Join-Path $root 'app'
$exe = Join-Path $appDir 'launcher.exe'
$data = Join-Path $appDir 'launcher-data'
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Launcher'
$keyBackup = Join-Path $root 'uninstall-key.reg'
$results = New-Object System.Collections.ArrayList

# The installer stops a running launcher.exe, and a second launcher would only hand
# over to the first: never on the owner's open launcher.
$running = Get-Process -Name launcher -ErrorAction SilentlyContinue
if ($running) {
  Write-Host '::error::A launcher is open on the test machine; close it and run again.'
  exit 2
}

Remove-Item -Recurse -Force $root -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $root | Out-Null
# The owner's own installation keeps its uninstall entry and shortcuts.
if (Test-Path $uninstallKey) {
  & reg.exe export 'HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Launcher' $keyBackup /y | Out-Null
}
$shortcuts = @(
  (Join-Path ([Environment]::GetFolderPath('Programs')) 'Launcher.lnk'),
  (Join-Path ([Environment]::GetFolderPath('Desktop')) 'Launcher.lnk')
) | Where-Object { Test-Path $_ }
$shortcutBackup = New-Item -ItemType Directory -Force (Join-Path $root 'shortcuts')
$shortcutIndex = 0
foreach ($link in $shortcuts) { Copy-Item $link (Join-Path $shortcutBackup "$shortcutIndex.lnk"); $shortcutIndex++ }

Say "Install $(Split-Path $Setup -Leaf) into its own folder"
$started = Get-Date
$install = Start-Process -FilePath $Setup -ArgumentList '/S', '/NS', "/D=$appDir" -Wait -PassThru
if ($install.ExitCode -ne 0 -or -not (Test-Path $exe)) {
  Write-Host "::error::The installer failed (exit $($install.ExitCode))."
  exit 1
}
Say "Installed in $([int]((Get-Date) - $started).TotalSeconds) s"
# Portable mode: the data folder beside the launcher, apart from the owner's.
Set-Content -Path (Join-Path $appDir 'portable.txt') -Value 'usertest'

$launcher = $null
function Restore-Machine {
  if ($script:launcher -and -not $script:launcher.HasExited) { Stop-Process -Id $script:launcher.Id -Force -ErrorAction SilentlyContinue }
  $uninstaller = Join-Path $appDir 'uninstall.exe'
  if (Test-Path $uninstaller) {
    Start-Process -FilePath $uninstaller -ArgumentList '/S', "_?=$appDir" -Wait | Out-Null
  }
  if (Test-Path $keyBackup) { & reg.exe import $keyBackup 2>&1 | Out-Null }
  $i = 0
  foreach ($link in $shortcuts) { Copy-Item (Join-Path $shortcutBackup "$i.lnk") $link -Force; $i++ }
}

try {
  Say 'Start the launcher'
  $launcher = Start-Process -FilePath $exe -PassThru
  $script:main = Wait-For { Find-Window 'Launcher' } 60
  if (-not $script:main) { throw 'the launcher window did not open' }
  Start-Sleep -Seconds 5

  # --- Accounts: an offline profile, as a player without a Microsoft account ----------
  Say 'Accounts: offline profile'
  Press $ui.navAccount 'Accounts tab'
  # Both switches (the launcher account's and the profiles without an account) to Offline.
  $offline = Wait-For { Find-Pressable $ui.modeOffline } 20
  if (-not $offline) { Say 'the Offline switch is not available' }
  foreach ($switch in @(Find-AllPressable $ui.modeOffline)) { Click $switch; Start-Sleep -Seconds 1 }
  $nameBox = Wait-For { Find-Input $ui.nameLabel } 20
  if (-not $nameBox) { throw 'the player name field was not found' }
  Type-Into $nameBox $Nick
  Press $ui.addProfile 'Add profile'
  Start-Sleep -Seconds 2
  $setup = New-Item -ItemType Directory -Force (Join-Path $Out 'setup')
  Dump-Launcher $setup

  foreach ($version in ($Versions -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ })) {
    $folder = New-Item -ItemType Directory -Force (Join-Path $Out $version)
    $result = [ordered]@{ version = $version; install = 'not run'; installSeconds = $null
      launch = 'not run'; launchSeconds = $null; world = 'not run'; worldSeconds = $null
      exit = 'not run'; exitCode = $null; logErrors = 0; note = '' }
    [void]$results.Add($result)
    Write-Host "::group::$version"
    try {
      # --- Installations: New installation, the version, Create, Install ----------------
      Say "$version : new installation"
      Press $ui.navHome 'Installations tab'
      Press $ui.create 'New installation'
      # The row reads "<id><tags><date>": the id ends where a letter, a space or the date starts.
      $pattern = "^$([regex]::Escape($version))(?=$|\s|[^\d.]|20\d\d-)"
      $row = Wait-For {
        foreach ($candidate in $script:main.FindAll($Scope::Descendants, $Any)) {
          try { if ($candidate.Current.IsKeyboardFocusable -and $candidate.Current.Name -match $pattern) { return $candidate } } catch { }
        }
      } 60
      if (-not $row) { $result.install = 'failed'; $result.note = 'the version is not in the launcher list'; throw 'version not listed' }
      Click $row
      Start-Sleep -Seconds 1
      $before = @(Get-ChildItem -Directory (Join-Path $data 'instances') -ErrorAction SilentlyContinue | ForEach-Object Name)
      Press $ui.createSubmit 'Create installation'
      $instance = Wait-For {
        Get-ChildItem -Directory (Join-Path $data 'instances') -ErrorAction SilentlyContinue |
          Where-Object { $before -notcontains $_.Name } | Select-Object -First 1
      } 30
      if (-not $instance) { $result.install = 'failed'; $result.note = 'the installation was not created'; throw 'not created' }

      Say "$version : install"
      $started = Get-Date
      Press $ui.install 'Install'
      $ready = $null
      $deadline = (Get-Date).AddMinutes(45)
      while ((Get-Date) -lt $deadline) {
        $ready = Find-Pressable $ui.play
        if ($ready) { break }
        $allow = Find-Pressable $ui.allow
        if ($allow) { Say 'the launcher asks to allow the download: allowed'; Click $allow }
        if (Find-Pressable $ui.retry) { break }
        Start-Sleep -Seconds 3
      }
      $result.installSeconds = [int]((Get-Date) - $started).TotalSeconds
      if (-not $ready) {
        $result.install = 'failed'; $result.note = 'install did not finish'
        Dump-Launcher $folder
        throw 'install failed'
      }
      $result.install = 'ok'
      Say "$version : installed in $($result.installSeconds) s"

      # --- Play ----------------------------------------------------------------------------
      $gameDir = Join-Path $instance.FullName 'game'
      $log = Join-Path $gameDir 'logs\latest.log'
      $shots = Join-Path $gameDir 'screenshots'
      $started = Get-Date
      Click $ready
      $game = Wait-For { Game-Window } 300
      if (-not $game) {
        $result.launch = 'failed'; $result.note = 'no game window in 5 minutes'
        Dump-Launcher $folder
        throw 'no game window'
      }
      $null = $game.Process.Handle  # keeps the exit code readable
      $up = Wait-For { $text = Log-Text $log; if ($text -match 'textures/atlas/|Sound engine started') { $true } } 300
      if (-not $up -or $game.Process.HasExited) {
        $result.launch = 'failed'; $result.note = 'the game did not finish loading'
        throw 'no title screen'
      }
      # The loading overlay fades out after the atlases.
      Start-Sleep -Seconds 15
      if ($game.Process.HasExited) { $result.launch = 'failed'; $result.note = 'the game closed after loading'; throw 'exited' }
      $result.launch = 'ok'
      $result.launchSeconds = [int]((Get-Date) - $started).TotalSeconds
      Say "$version : title screen after $($result.launchSeconds) s ($($game.Title))"
      $hwnd = $game.Hwnd
      # First start: the accessibility welcome screen closes with Escape; the title
      # screen ignores it.
      Game-Key $hwnd 'Escape'
      Start-Sleep -Seconds 2
      Game-Key $hwnd 'F2'

      # --- Singleplayer -> Create New World --------------------------------------------------
      $layout = Gui-Layout $hwnd
      Say "$version : window $($layout.Width)x$($layout.Height), GUI scale $($layout.Scale)"
      $started = Get-Date
      $joined = "$([regex]::Escape($Nick)) joined the game"
      # Singleplayer: the first button under the logo, 200x20 at height/4 + 48.
      Game-Click $hwnd ($layout.GuiWidth / 2) ([math]::Floor($layout.GuiHeight / 4) + 48 + 10)
      Start-Sleep -Seconds 4
      Game-Key $hwnd 'F2'
      # With no worlds the game opens Create New World at once; its Create button is the
      # left of two 150-wide buttons in the footer.
      $createX = [math]::Floor(($layout.GuiWidth - 308) / 2) + 75
      Game-Click $hwnd $createX ($layout.GuiHeight - 17)
      $inWorld = Wait-For { if ((Log-Text $log) -match $joined) { $true } } 180
      if (-not $inWorld) {
        # The keyboard way: Tab through the buttons is version dependent, so the
        # screen is shot and Enter tried on the focused button.
        Game-Key $hwnd 'F2'
        Say "$version : no world after the clicks; trying the keyboard"
        Game-Key $hwnd 'Tab'; Game-Key $hwnd 'Enter'
        $inWorld = Wait-For { if ((Log-Text $log) -match $joined) { $true } } 120
      }
      if (-not $inWorld) {
        $result.world = 'failed'; $result.note = 'did not get into a world'
        Game-Key $hwnd 'F2'
      } else {
        $result.world = 'ok'
        $result.worldSeconds = [int]((Get-Date) - $started).TotalSeconds
        Say "$version : in the world after $($result.worldSeconds) s"
        # A look around: the game pauses when its window is not in front, so the
        # pause menu is closed first.
        Start-Sleep -Seconds 10
        Game-Key $hwnd 'Escape'
        Start-Sleep -Seconds 10
        Game-Key $hwnd 'F2'
        Start-Sleep -Seconds 10
      }

      # --- Close the game window, as a player does ------------------------------------
      [UserTest.Win]::PostMessage($hwnd, $WM_CLOSE, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
      if (-not $game.Process.WaitForExit(120000)) {
        $result.exit = 'hung'; $result.note = "$($result.note); the game did not close in 2 minutes".Trim('; ')
        Stop-Process -Id $game.Process.Id -Force -ErrorAction SilentlyContinue
      } else {
        $result.exitCode = $game.Process.ExitCode
        $result.exit = if ($result.exitCode -eq 0) { 'ok' } else { 'failed' }
      }
      $crashes = @(Get-ChildItem (Join-Path $gameDir 'crash-reports') -ErrorAction SilentlyContinue)
      if ($crashes) { $result.exit = 'crashed'; $result.note = "$($result.note); crash report".Trim('; ') }
      Start-Sleep -Seconds 3
    } catch {
      Say "$version : $($_.Exception.Message)"
      if ($result.note -eq '') { $result.note = $_.Exception.Message }
      $leftover = Game-Window
      if ($leftover) { Stop-Process -Id $leftover.Process.Id -Force -ErrorAction SilentlyContinue }
    } finally {
      if ($gameDir -and (Test-Path $log)) {
        $text = Log-Text $log
        $result.logErrors = ([regex]::Matches($text, '/(ERROR|FATAL)\]')).Count
        [IO.File]::WriteAllText((Join-Path $folder 'latest.log'), (Scrub $text), (New-Object Text.UTF8Encoding $false))
      }
      if ($gameDir) {
        Get-ChildItem $shots -Filter *.png -ErrorAction SilentlyContinue | Copy-Item -Destination $folder
        Get-ChildItem (Join-Path $gameDir 'crash-reports') -ErrorAction SilentlyContinue | ForEach-Object {
          [IO.File]::WriteAllText((Join-Path $folder $_.Name), (Scrub ([IO.File]::ReadAllText($_.FullName))))
        }
      }
      Write-Host "::endgroup::"
      Say ("$version : install {0}, launch {1}, world {2}, exit {3} {4}" -f $result.install, $result.launch, $result.world, $result.exit, $result.note)
      $gameDir = $null
    }
  }
} catch {
  Say "stopped: $($_.Exception.Message)"
  if ($script:main) { Dump-Launcher $Out }
  [void]$results.Add([ordered]@{ version = '(launcher)'; note = $_.Exception.Message })
} finally {
  $results | ConvertTo-Json -Depth 4 | Set-Content -Encoding UTF8 (Join-Path $Out 'results.json')
  Restore-Machine
}

$failed = @($results | Where-Object { $_.world -ne 'ok' -or $_.exit -ne 'ok' })
if ($failed) { exit 1 }

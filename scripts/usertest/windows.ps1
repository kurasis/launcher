# Player's-eye check of game versions on the Windows test machine (docs/USERTEST.md).
# Usage: powershell -NoProfile -ExecutionPolicy Bypass -File windows.ps1 `
#          -Setup <setup.exe> -Strings <ui-strings.json> -Items vanilla:26.1,forge:1.20.1 -Out <folder> `
#          [-Special <special.json>]
#
# Installs the packaged launcher into its own folder in portable mode, then for each
# item (a loader, or vanilla, and a game version) does what a player does: Accounts ->
# offline profile, Installations -> New installation -> the loader -> the version ->
# Create -> Install -> Play; in the game Singleplayer ->
# Create New World, a look around, and the window closed. A special version (special:<id>,
# the launcher's own list, D-073) is chosen on the form's Special tab and played to its
# title screen only. The launcher is driven through
# UI Automation, the game with the mouse and keys as a player uses them (its window brought
# to the front), so the test machine needs a signed-in, unlocked desktop. Nothing is
# changed in the game files or settings. Results go to <Out>/results.json; screenshots and the
# game's log for a failure (machine names replaced) to <Out>/<loader>-<version>/.
#
# Plain ASCII only: Windows PowerShell reads this file in the ANSI code page. The
# interface texts come from ui-strings.json (every catalog's text for each key).
param(
  [string]$Setup,
  [string]$Strings,
  [string]$Items,
  [string]$Out,
  # The special versions of the plan: { "<id>": { "name": ..., "selected": ... } }.
  [string]$Special,
  [string]$Nick = 'Tester',
  # Settings, Network: where downloads come from.
  [ValidateSet('automatic', 'mirror_first', 'origin_only')]
  [string]$Downloads = 'automatic',
  # Only puts the machine back: stops and uninstalls what an earlier run left.
  [switch]$CleanupOnly
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
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, IntPtr extra);
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr value);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, IntPtr extra);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr hwnd, ref POINT point);
    [DllImport("user32.dll")] public static extern uint MapVirtualKey(uint code, uint mapType);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int index);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }

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
if (-not $CleanupOnly) {
  foreach ($name in 'Setup', 'Strings', 'Items', 'Out') {
    if (-not (Get-Variable $name -ValueOnly)) { Write-Host "::error::-$name is needed"; exit 2 }
  }
  $ui = [IO.File]::ReadAllText($Strings, [Text.Encoding]::UTF8) | ConvertFrom-Json
  New-Item -ItemType Directory -Force $Out | Out-Null
  # A special version's row starts with its name, and the form says "Selected: <name>"
  # (an earlier build: its id).
  $specials = @{}
  if ($Special) {
    $listed = [IO.File]::ReadAllText($Special, [Text.Encoding]::UTF8) | ConvertFrom-Json
    foreach ($entry in $listed.PSObject.Properties) { $specials[$entry.Name] = $entry.Value }
  }
  # "<loader>:<version>" or "special:<id>" each; anything else stops the run before it
  # touches the machine.
  $plan = @($Items -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ })
  foreach ($item in $plan) {
    if ($item -match '^special:([A-Za-z0-9][A-Za-z0-9 ._\-]{0,63})$') {
      if (-not $specials.ContainsKey($Matches[1])) { Write-Host "::error::not in the special versions: $item"; exit 2 }
    } elseif ($item -notmatch '^(vanilla|fabric|quilt|forge|neoforge):[0-9][0-9a-z.\-]{0,31}$') { Write-Host "::error::not an item: $item"; exit 2 }
  }
  # The loader's name as the form offers it (the same in every language).
  $loaderNames = @{ vanilla = ''; special = ''; fabric = $ui.loaderFabric[0]; quilt = $ui.loaderQuilt[0]
    forge = $ui.loaderForge[0]; neoforge = $ui.loaderNeoForge[0] }
}

# Log lines that show the game window is up, as the launcher knows them
# (src-tauri/src/launch/logs.rs READY_MARKERS).
$readyMarkers = @('minecraft:textures/atlas/', 'textures-atlas', 'Sound engine started',
  'Reloading ResourceManager', 'Launching wrapped minecraft')

# Game versions compared by their numbers: 1.12.2 < 1.19.4 < 26.1.
function Compare-Version([string]$A, [string]$B) {
  $x = @($A -split '\.' | ForEach-Object { [int]($_ -replace '\D.*$', '0') })
  $y = @($B -split '\.' | ForEach-Object { [int]($_ -replace '\D.*$', '0') })
  for ($i = 0; $i -lt [math]::Max($x.Count, $y.Count); $i++) {
    $p = if ($i -lt $x.Count) { $x[$i] } else { 0 }
    $q = if ($i -lt $y.Count) { $y[$i] } else { 0 }
    if ($p -ne $q) { return [math]::Sign($p - $q) }
  }
  return 0
}

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
# CSS-transformed text. A pattern ending in '*' matches a name's start. Bidi isolates
# (around a special version's name in "Selected: ...") do not count.
function Matches([string]$Name, $Texts) {
  $Name = $Name -replace '[\u2066-\u2069]', ''
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

# The mod loader: a row of radio buttons (pills, D-072), each named by its loader.
function Find-Radio($Texts) {
  $radio = [System.Windows.Automation.ControlType]::RadioButton
  $byType = New-Object System.Windows.Automation.PropertyCondition($AE::ControlTypeProperty, $radio)
  foreach ($candidate in $script:main.FindAll($Scope::Descendants, $byType)) {
    if (Matches $candidate.Current.Name $Texts) { return $candidate }
  }
  return $null
}

function Is-Selected($Element) {
  $pattern = $null
  if ($Element.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern, [ref]$pattern)) { return $pattern.Current.IsSelected }
  return $false
}

# Picks the loader's pill as a player does; if that does not take, the radio focused
# and Space pressed.
function Choose-Radio($Radio) {
  Click $Radio
  Start-Sleep -Milliseconds 700
  if (Is-Selected $Radio) { return $true }
  Add-Type -AssemblyName System.Windows.Forms
  try { $Radio.SetFocus() } catch { }
  [System.Windows.Forms.SendKeys]::SendWait(' ')
  Start-Sleep -Seconds 1
  return (Is-Selected $Radio)
}

# Drop-down lists (HTML select): the loader version.
function Find-AllCombos {
  $combo = [System.Windows.Automation.ControlType]::ComboBox
  $byType = New-Object System.Windows.Automation.PropertyCondition($AE::ControlTypeProperty, $combo)
  return @($script:main.FindAll($Scope::Descendants, $byType))
}

function Find-Combo($Texts) {
  foreach ($candidate in Find-AllCombos) {
    if (Matches $candidate.Current.Name $Texts) { return $candidate }
  }
  return $null
}

function Combo-Value($Combo) {
  $pattern = $null
  if ($Combo.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) { return $pattern.Current.Value }
  return ''
}

# Picks an option as a player does: the list opened and the option chosen, or, if that
# does not take, the option's name typed on the closed list (the browser's type-ahead).
function Choose-Option($Combo, [string]$Option) {
  $expand = $null
  if ($Combo.TryGetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern, [ref]$expand)) {
    try { $expand.Expand(); Start-Sleep -Milliseconds 500 } catch { }
  }
  foreach ($candidate in $Combo.FindAll($Scope::Descendants, $Any)) {
    try {
      if ($candidate.Current.Name -ieq $Option) {
        $pattern = $null
        if ($candidate.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern, [ref]$pattern)) { $pattern.Select() }
        elseif ($candidate.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) { $pattern.Invoke() }
        break
      }
    } catch { }
  }
  if ($expand) { try { $expand.Collapse() } catch { } }
  Start-Sleep -Milliseconds 700
  if ((Combo-Value $Combo) -ieq $Option) { return $true }
  Add-Type -AssemblyName System.Windows.Forms
  try { $Combo.SetFocus() } catch { }
  [System.Windows.Forms.SendKeys]::SendWait($Option)
  Start-Sleep -Seconds 1
  return (Combo-Value $Combo) -ieq $Option
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
  if ($Element.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$pattern)) {
    $pattern.Toggle()
    return
  }
  if ($Element.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern, [ref]$pattern)) {
    $pattern.Select()
    return
  }
  Mouse-Click $Element
}

# A real click in the middle of the element, scrolled into view first.
function Mouse-Click($Element) {
  $pattern = $null
  if ($Element.TryGetCurrentPattern([System.Windows.Automation.ScrollItemPattern]::Pattern, [ref]$pattern)) {
    try { $pattern.ScrollIntoView(); Start-Sleep -Milliseconds 300 } catch { }
  }
  try { $script:main.SetFocus() } catch { }
  $r = $Element.Current.BoundingRectangle
  [UserTest.Win]::SetCursorPos([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2)) | Out-Null
  Start-Sleep -Milliseconds 100
  [UserTest.Win]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero)
  [UserTest.Win]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero)
}

# The launcher's main window. It is closed while a game runs and made anew after it, so
# it is looked up again before each version; a start of launcher.exe brings it back if
# it is gone (the running launcher shows its window).
function Get-Launcher {
  $window = Wait-For { Find-Window 'Launcher' } 60
  if (-not $window) {
    Say 'the launcher window is gone: starting launcher.exe again'
    Start-Process -FilePath $exe | Out-Null
    $window = Wait-For { Find-Window 'Launcher' } 60
  }
  if (-not $window) { throw 'the launcher window did not come back' }
  $pattern = $null
  if ($window.TryGetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern, [ref]$pattern)) {
    try {
      if ($pattern.Current.WindowVisualState -eq [System.Windows.Automation.WindowVisualState]::Minimized) {
        $pattern.SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Normal)
      }
    } catch { }
  }
  try { $window.SetFocus() } catch { }
  $script:main = $window
  Start-Sleep -Seconds 2
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

# The end of the launcher's own log, for a failed install: machine names and any IP
# address replaced (the repository is public).
function Launcher-Log([string]$Folder) {
  $file = Join-Path $data 'logs\launcher.log'
  if (-not (Test-Path $file)) { return }
  $lines = @((Log-Text $file) -split "`r?`n" | Select-Object -Last 150)
  $text = (Scrub ($lines -join "`n")) -replace '\b\d{1,3}(\.\d{1,3}){3}\b', '<ip>' -replace '\b[0-9a-fA-F]{1,4}(:[0-9a-fA-F]{0,4}){3,7}\b', '<ip>'
  [IO.File]::WriteAllText((Join-Path $Folder 'launcher-log.txt'), $text, (New-Object Text.UTF8Encoding $false))
}

# --- The game, with the mouse and the keyboard ------------------------------------------
#
# 26.x reads input through SDL3, which takes no posted mouse messages and keys only while
# its window has the focus. So the window is brought to the front and real input is sent:
# the cursor is moved onto a button and clicked, keys are pressed as a keyboard does.

$WM_KEYDOWN = 0x0100; $WM_KEYUP = 0x0101; $WM_SYSCOMMAND = 0x0112; $SC_CLOSE = 0xF060
$Keys = @{ Escape = 0x1B; Enter = 0x0D; Tab = 0x09; F2 = 0x71 }

# Brings the game window to the front (Windows allows it after an Alt press); false on a
# locked screen or when Windows refuses.
function Game-Focus([IntPtr]$Hwnd) {
  if ([UserTest.Win]::GetForegroundWindow() -eq $Hwnd) { return $true }
  Close-DriverReports
  for ($try = 0; $try -lt 3; $try++) {
    [UserTest.Win]::keybd_event(0x12, 0, 0, [IntPtr]::Zero)
    [UserTest.Win]::keybd_event(0x12, 0, 2, [IntPtr]::Zero)
    [UserTest.Win]::SetForegroundWindow($Hwnd) | Out-Null
    Start-Sleep -Milliseconds 400
    if ([UserTest.Win]::GetForegroundWindow() -eq $Hwnd) { return $true }
  }
  return $false
}

# A key press; posted to the window only when it could not be brought to the front.
function Game-Key([IntPtr]$Hwnd, [string]$Key) {
  $vk = $Keys[$Key]
  $sc = [UserTest.Win]::MapVirtualKey($vk, 0)
  if (Game-Focus $Hwnd) {
    [UserTest.Win]::keybd_event([byte]$vk, [byte]$sc, 0, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 120
    [UserTest.Win]::keybd_event([byte]$vk, [byte]$sc, 2, [IntPtr]::Zero)
  } else {
    [UserTest.Win]::PostMessage($Hwnd, $WM_KEYDOWN, [IntPtr]$vk, [IntPtr][int64](1 -bor ($sc -shl 16))) | Out-Null
    Start-Sleep -Milliseconds 120
    $up = [int64]1 -bor ([int64]$sc -shl 16) -bor ([int64]1 -shl 30) -bor ([int64]1 -shl 31)
    [UserTest.Win]::PostMessage($Hwnd, $WM_KEYUP, [IntPtr]$vk, [IntPtr]$up) | Out-Null
  }
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

# A picture of the game window: the game's own F2 screenshot when the window has the
# focus, and a copy of the screen region into the results.
$script:shot = 0
function Game-Shot([IntPtr]$Hwnd, [string]$Folder, [string]$Name) {
  $script:shot++
  if (Game-Focus $Hwnd) { Game-Key $Hwnd 'F2' }
  try {
    $rect = New-Object UserTest.Win+RECT
    [UserTest.Win]::GetWindowRect($Hwnd, [ref]$rect) | Out-Null
    $w = $rect.Right - $rect.Left; $h = $rect.Bottom - $rect.Top
    $bitmap = New-Object System.Drawing.Bitmap($w, $h)
    [System.Drawing.Graphics]::FromImage($bitmap).CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bitmap.Size)
    $bitmap.Save((Join-Path $Folder ('screen-{0:D2}-{1}.jpg' -f $script:shot, $Name)), [System.Drawing.Imaging.ImageFormat]::Jpeg)
  } catch { Say "no screen copy: $($_.Exception.Message)" }
}

# A left click at GUI coordinates: the window to the front, the cursor onto the point
# (screen pixels), a small move there as a hand makes, then the button.
function Game-Click([IntPtr]$Hwnd, [double]$GuiX, [double]$GuiY) {
  if (-not (Game-Focus $Hwnd)) { Say 'the game window could not be brought to the front' }
  $layout = Gui-Layout $Hwnd
  $point = New-Object UserTest.Win+POINT
  $point.X = [int]($GuiX * $layout.Scale); $point.Y = [int]($GuiY * $layout.Scale)
  [UserTest.Win]::ClientToScreen($Hwnd, [ref]$point) | Out-Null
  [UserTest.Win]::SetCursorPos($point.X - 4, $point.Y - 2) | Out-Null
  Start-Sleep -Milliseconds 150
  [UserTest.Win]::SetCursorPos($point.X, $point.Y) | Out-Null
  [UserTest.Win]::mouse_event(0x0001, 0, 0, 0, [IntPtr]::Zero)
  # Before 1.4.6 the game takes in the window's messages just before it draws a frame,
  # and the world list drops the mouse events a frame sees, so a click is gone before a
  # tick reads it. While F7 is held those versions take the messages in after the frame
  # instead (their own debugging key), so the next tick, when one is due, gets the click.
  $f7 = (Compare-Version $version '1.4.6') -lt 0
  if ($f7) { [UserTest.Win]::keybd_event(0x76, 0x41, 0, [IntPtr]::Zero) }
  # A random part of a tick, so that a run of clicks does not keep the same step with
  # the game's 20 ticks a second.
  Start-Sleep -Milliseconds (400 + (Get-Random -Maximum 50))
  # Held for a quarter second: versions before 1.7 read button presses only on their
  # 20-per-second ticks, and their world list drops what a frame sees after release.
  [UserTest.Win]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero)
  Start-Sleep -Milliseconds 250
  [UserTest.Win]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero)
  if ($f7) { Start-Sleep -Milliseconds 200; [UserTest.Win]::keybd_event(0x76, 0x41, 2, [IntPtr]::Zero) }
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

# Launcher Skins, the launcher's own skins mod, for the run just played: '' when the
# launcher has no build of it for this version, 'ok' when the game took it, 'not added'
# when the launcher could not add it, 'not loaded' when the game's logs never name it,
# 'failed' when they name it with an error. A loader names the mod id in its mod list
# (Fabric, Quilt), its mod file scan (Forge, NeoForge: debug.log) or its connection
# handshake (Forge 1.7.10, 1.12.2); the Java agent of the game without a loader writes
# only when it cannot hook in.
function Skins-State([string]$Loader, [int]$From, [string]$GameDir, [string]$RunLog) {
  $launcherText = Log-Text (Join-Path $data 'logs\launcher.log')
  if ($launcherText.Length -lt $From) { $From = 0 }
  $since = $launcherText.Substring($From)
  if ($since -match '(?m)\swarn\s+Launcher Skins (could not|cannot)') { return 'not added' }
  if (-not $since.Contains('Launcher Skins added to the game')) { return '' }
  $text = ''
  if ($GameDir) {
    foreach ($f in @(Get-ChildItem (Join-Path $GameDir 'logs') -Filter *.log -ErrorAction SilentlyContinue)) { $text += (Log-Text $f.FullName) + "`n" }
  }
  if ($RunLog) { $text += Log-Text $RunLog }
  if ($text -match "Launcher Skins: (the agent's targets were not found|PlayerInfo left unchanged)" -or
      $text -match '(?im)^.*launcherskins.*(ERROR|FATAL|exception|failed|invalid|incompatible|could not).*$' -or
      $text -match '(?im)^.*(ERROR|FATAL|exception|failed|invalid|incompatible|could not).*launcherskins.*$') { return 'failed' }
  if ($Loader -eq 'vanilla' -or $text -match '(?i)launcherskins') { return 'ok' }
  return 'not loaded'
}

# --- A graphics driver crash: neither the launcher's nor the game's hang ------------
# Windows logs a display driver that stopped responding and was restarted (TDR) as
# System event 4101, and its dump as a LiveKernelEvent of Windows Error Reporting. The
# AMD driver then opens its bug report window, which also takes the focus from the game.
function Driver-Reports {
  Get-Process -ErrorAction SilentlyContinue | Where-Object {
    $_.ProcessName -match '(?i)BugReport' -or $_.MainWindowTitle -match '(?i)Bug Report Tool|AMD Bug Report|driver timeout' }
}

function Close-DriverReports {
  foreach ($p in @(Driver-Reports)) {
    Say "closing the graphics driver's report window ($($p.ProcessName))"
    try { [void]$p.CloseMainWindow() } catch { }
    if ($p.ProcessName -match '(?i)BugReport' -and -not $p.WaitForExit(5000)) {
      Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    }
  }
}

function Driver-Crashed([datetime]$Since) {
  if (@(Get-WinEvent -FilterHashtable @{ LogName = 'System'; Id = 4101; StartTime = $Since } -ErrorAction SilentlyContinue).Count) { return $true }
  if (@(Get-WinEvent -FilterHashtable @{ LogName = 'Application'; Id = 1001; StartTime = $Since } -ErrorAction SilentlyContinue |
      Where-Object { $_.Message -match 'LiveKernelEvent' }).Count) { return $true }
  return @(Driver-Reports).Count -gt 0
}

# An item passes with its world entered and the game closed normally, and Launcher Skins
# in where it is added. A special version passes at its title screen, without Launcher
# Skins (the launcher adds none to these).
function Passed($Result) {
  if ($Result.exit -ne 'ok' -or $Result.skins -match '^not|^failed') { return $false }
  if ($Result.loader -eq 'special') { return $Result.launch -eq 'ok' -and -not $Result.skins }
  return $Result.world -eq 'ok'
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

$shortcutBackup = Join-Path $root 'shortcuts'
$shortcutList = Join-Path $root 'shortcuts.txt'

# The game files every installation shares (libraries, assets: content-addressed and
# checked by the launcher before use) are kept between the parts a machine plays, so a
# long run does not fetch the same gigabytes again for each part. Up to 25 GiB; the
# cleanup run removes them.
$cache = Join-Path $env:LOCALAPPDATA 'launcher-usertest-cache'
function Keep-Shared {
  $shared = Join-Path $data 'shared'
  if (-not (Test-Path $shared)) { return }
  $bytes = (Get-ChildItem $shared -Recurse -File -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum
  Remove-Item -Recurse -Force $cache -ErrorAction SilentlyContinue
  if ($bytes -lt 25GB) {
    New-Item -ItemType Directory -Force $cache | Out-Null
    try { Move-Item $shared (Join-Path $cache 'shared') } catch { Say "shared files not kept: $($_.Exception.Message)" }
  }
}

# Puts back what a run changed outside its folder: uninstalls the test launcher, returns
# the owner's uninstall entry and shortcuts (saved in the folder before the install).
function Restore-Machine {
  if ($script:launcher -and -not $script:launcher.HasExited) { Stop-Process -Id $script:launcher.Id -Force -ErrorAction SilentlyContinue }
  if (-not $CleanupOnly) { Start-Sleep -Seconds 2; Keep-Shared }
  $uninstaller = Join-Path $appDir 'uninstall.exe'
  if (Test-Path $uninstaller) {
    Start-Process -FilePath $uninstaller -ArgumentList '/S', "_?=$appDir" -Wait | Out-Null
  }
  if (Test-Path $keyBackup) { & reg.exe import $keyBackup 2>&1 | Out-Null }
  if (Test-Path $shortcutList) {
    $i = 0
    foreach ($link in @(Get-Content $shortcutList)) {
      $saved = Join-Path $shortcutBackup "$i.lnk"
      if ($link -and (Test-Path $saved)) { Copy-Item $saved $link -Force }
      $i++
    }
  }
}

# What an earlier, cancelled run left: its processes (the launcher, its Java) are
# stopped and the machine put back; nothing outside its folder is touched.
Get-Process -ErrorAction SilentlyContinue | Where-Object {
  try { $_.Path -and $_.Path.StartsWith($root + '\', [StringComparison]::OrdinalIgnoreCase) } catch { $false }
} | ForEach-Object { Say "stopping $($_.ProcessName) left by an earlier run"; Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue }
Start-Sleep -Seconds 2
if (Test-Path $root) {
  Say 'putting back what an earlier run left'
  Restore-Machine
  if ((Test-Path $shortcutBackup) -and -not (Test-Path $shortcutList) -and (Get-ChildItem $shortcutBackup)) {
    # A run before the list was kept: the saved shortcuts stay for the owner.
    $kept = Join-Path $env:LOCALAPPDATA 'launcher-usertest-shortcuts'
    Copy-Item -Recurse -Force $shortcutBackup $kept
    Write-Host "::warning::Saved Launcher shortcuts of an earlier run are in %LOCALAPPDATA%\launcher-usertest-shortcuts."
  }
  Remove-Item -Recurse -Force $root -ErrorAction SilentlyContinue
}
if ($CleanupOnly) {
  Remove-Item -Recurse -Force $cache -ErrorAction SilentlyContinue
  if ((Test-Path $root) -or (Test-Path $cache)) { Write-Host '::error::The test folder could not be removed.'; exit 1 }
  Say 'The machine is clean'
  exit 0
}

# The installer stops a running launcher.exe, and a second launcher would only hand
# over to the first: never on the owner's open launcher.
$running = Get-Process -Name launcher -ErrorAction SilentlyContinue
if ($running) {
  Write-Host '::error::A launcher is open on the test machine; close it and run again.'
  exit 2
}

New-Item -ItemType Directory -Force $root | Out-Null
# The owner's own installation keeps its uninstall entry and shortcuts.
if (Test-Path $uninstallKey) {
  & reg.exe export 'HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Launcher' $keyBackup /y | Out-Null
}
$shortcuts = @(
  (Join-Path ([Environment]::GetFolderPath('Programs')) 'Launcher.lnk'),
  (Join-Path ([Environment]::GetFolderPath('Desktop')) 'Launcher.lnk')
) | Where-Object { Test-Path $_ }
New-Item -ItemType Directory -Force $shortcutBackup | Out-Null
$shortcutIndex = 0
foreach ($link in $shortcuts) { Copy-Item $link (Join-Path $shortcutBackup "$shortcutIndex.lnk"); $shortcutIndex++ }
Set-Content -Path $shortcutList -Value @($shortcuts)

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
if (Test-Path (Join-Path $cache 'shared')) {
  New-Item -ItemType Directory -Force $data | Out-Null
  try { Move-Item (Join-Path $cache 'shared') (Join-Path $data 'shared'); Say 'shared game files of earlier parts in place' } catch { Say "shared files not reused: $($_.Exception.Message)" }
}

if ($Downloads -ne 'automatic') {
  # The choice a player makes in Settings, Network, written before the first start; the
  # launcher fills in the rest of its settings.
  New-Item -ItemType Directory -Force $data | Out-Null
  $utf8 = New-Object Text.UTF8Encoding $false
  [IO.File]::WriteAllText((Join-Path $data 'settings.json'), "{`"schemaVersion`":1,`"downloadSource`":`"$Downloads`"}", $utf8)
}
Say "Downloads: $Downloads"
# Old games (LWJGL 2) take a mouse button as held only when Windows counts mouse
# buttons, and with no mouse plugged in it may count none: noted for a failed click.
Say "Mouse buttons Windows counts: $([UserTest.Win]::GetSystemMetrics(43))"

$launcher = $null

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

  # An item the graphics driver crashed under is played once more, at the end.
  $queue = [Collections.ArrayList]@($plan)
  $replayed = @{}
  for ($n = 0; $n -lt $queue.Count; $n++) {
    $item = $queue[$n]
    $loader, $version = $item -split ':', 2
    $isSpecial = $loader -eq 'special'
    $modded = [bool]$loaderNames[$loader]
    $label = if ($loader -eq 'vanilla') { $version } elseif ($isSpecial) { "$version (special)" } else { "$version $($loaderNames[$loader])" }
    $folder = New-Item -ItemType Directory -Force (Join-Path $Out "$loader-$version")
    $result = [ordered]@{ item = $item; loader = $loader; version = $version; loaderVersion = ''
      machine = $env:RUNNER_NAME; install = 'not run'; installSeconds = $null
      launch = 'not run'; launchSeconds = $null; world = 'not run'; worldSeconds = $null
      exit = 'not run'; exitCode = $null; skins = ''; driver = ''; logErrors = 0; note = '' }
    [void]$results.Add($result)
    $gameDir = $null; $runLog = $null; $keep = $false; $launcherLogFrom = $null; $itemStarted = Get-Date
    Close-DriverReports
    Write-Host "::group::$label"
    try {
      # --- Installations: New installation, the loader, the version, Create, Install ---------
      Say "$label : new installation"
      Get-Launcher
      Press $ui.navHome 'Installations tab'
      # A form left open by an item that stopped halfway is closed first.
      $open = Find-Pressable $ui.cancel
      if ($open) { Click $open; Start-Sleep -Seconds 1 }
      Press $ui.create 'New installation'
      if ($modded) {
        $pill = Wait-For { Find-Radio $loaderNames[$loader] } 20
        if (-not $pill) { $result.install = 'failed'; $result.note = 'no loader choice in the form'; throw 'no loader pill' }
        if (-not (Choose-Radio $pill)) {
          $result.install = 'failed'; $result.note = "$($loaderNames[$loader]) could not be chosen"
          throw 'loader not chosen'
        }
        # The version list then shows only what the loader supports.
        Start-Sleep -Seconds 2
      }
      if ($isSpecial) {
        # The launcher's own versions are on a tab of their own, after Alpha; its name
        # ends with the count. Only the chosen tab is in the tab order.
        $tabTexts = @($ui.special | ForEach-Object { "$_*" })
        $byTab = New-Object System.Windows.Automation.PropertyCondition($AE::ControlTypeProperty, [System.Windows.Automation.ControlType]::TabItem)
        $tab = Wait-For {
          foreach ($candidate in $script:main.FindAll($Scope::Descendants, $byTab)) {
            try { if (Matches $candidate.Current.Name $tabTexts) { return $candidate } } catch { }
          }
        } 30
        if (-not $tab) { $result.install = 'failed'; $result.note = 'no Special tab: the launcher shows no special versions'; throw 'no special tab' }
        Click $tab
        Start-Sleep -Seconds 1
        $selection = $null
        if ($tab.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern, [ref]$selection) -and -not $selection.Current.IsSelected) {
          Mouse-Click $tab
          Start-Sleep -Seconds 1
        }
      }
      # As a player finds a version: typed into the search box above the list (it finds
      # a special version by its id too).
      $search = Wait-For { Find-Input $ui.search } 20
      if ($search) { Type-Into $search $version; Start-Sleep -Seconds 1 } else { Say 'no version search box' }
      # A version row is a button that reads "<id><tags><date>", the date in the
      # interface language ("Apr 1, 2026"; some languages put the day first), so it is
      # told by its first text, the id itself (1.21 is not 1.21.1), and a year after it.
      # A special version's row starts with its name, which an earlier build shares with
      # the build Mojang kept. A row with no tag can show no texts of its own to UI
      # Automation; it is taken by its name, and the check of the choice below skips a
      # row that turns out to be another version.
      $shown = if ($isSpecial) { $specials[$version] } else { [pscustomobject]@{ name = $version; selected = $version } }
      $rows = Wait-For {
        foreach ($candidate in $script:main.FindAll($Scope::Descendants, $Any)) {
          try {
            $c = $candidate.Current
            if (-not $c.IsKeyboardFocusable -or -not $c.Name.StartsWith($shown.name, [StringComparison]::OrdinalIgnoreCase)) { continue }
            if ($c.Name.Substring($shown.name.Length) -notmatch '\d{4}') { continue }
            $first = @($candidate.FindAll($Scope::Descendants, $Any) | Where-Object { $_.Current.Name } | Select-Object -First 1)
            if (-not $first -or $first[0].Current.Name -ieq $shown.name) { $candidate }
          } catch { }
        }
      } 60
      if (-not $rows) { $result.install = 'failed'; $result.note = 'the version is not in the launcher list'; throw 'version not listed' }
      # The form says which version is chosen ("Selected: <id>"); the newest is chosen
      # at first, so the choice is checked before the installation is made. Of rows
      # with the same name, the one that the form names.
      $chosen = @($ui.selected | ForEach-Object { $_.Replace('{id}', $shown.selected) })
      $isChosen = { if (@(Get-Texts | Where-Object { Matches $_ $chosen })) { $true } }
      foreach ($row in @($rows)) {
        Click $row
        if (-not (Wait-For $isChosen 5)) {
          Say "$label : the row click did not choose it; clicking with the mouse"
          Mouse-Click $row
        }
        if (Wait-For $isChosen 5) { break }
      }
      if (-not (Wait-For $isChosen 1)) {
        $result.install = 'failed'; $result.note = 'the version could not be chosen in the form'
        throw 'version not chosen'
      }
      $before = @(Get-ChildItem -Directory (Join-Path $data 'instances') -ErrorAction SilentlyContinue | ForEach-Object Name)
      # With a loader the button waits for the loader's versions (the newest
      # recommended or stable one is taken, as a player leaves it).
      $submit = Wait-For { Find-Pressable $ui.createSubmit } 120
      if (-not $submit) {
        $result.install = 'failed'
        $result.note = if (-not $modded) { 'Create installation stayed unavailable' } else { "no $($loaderNames[$loader]) version offered" }
        throw 'cannot create'
      }
      if ($modded) {
        $picked = @(Find-AllCombos | Where-Object { -not (Matches $_.Current.Name $ui.loader) } | ForEach-Object { Combo-Value $_ } | Where-Object { $_ })
        if ($picked) { $result.loaderVersion = $picked[0] }
      }
      Click $submit
      Start-Sleep -Milliseconds 700
      $instance = Wait-For {
        Get-ChildItem -Directory (Join-Path $data 'instances') -ErrorAction SilentlyContinue |
          Where-Object { $before -notcontains $_.Name } | Select-Object -First 1
      } 30
      if (-not $instance) { $result.install = 'failed'; $result.note = 'the installation was not created'; throw 'not created' }

      Say "$label : install"
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
      if (-not $ready) { $result.install = 'failed'; $result.note = 'install did not finish'; throw 'install failed' }
      $result.install = 'ok'
      Say "$label : installed in $($result.installSeconds) s"

      # --- Play ----------------------------------------------------------------------------
      $gameDir = Join-Path $instance.FullName 'game'
      if (-not $isSpecial -and (Compare-Version $version '1.7') -lt 0) {
        # Before 1.7 the world list reads and drops every mouse event of a frame drawn
        # without the button down, which loses the test's clicks on its buttons; the
        # game's own touchscreen mode skips that. Written before the first start.
        # Touchscreen mode came in 1.4.6: before it a click counts only when a tick
        # reads it before the next frame is drawn (Game-Click holds F7 for that), so
        # those versions get the power saver frame limit (40 frames a second in menus for
        # 20 ticks, so a tick is due about every second frame) and more tries.
        New-Item -ItemType Directory -Force $gameDir | Out-Null
        $options = Join-Path $gameDir 'options.txt'
        $lines = if ((Compare-Version $version '1.4.6') -lt 0) { @('touchscreen:true', 'fpsLimit:2') } else { @('touchscreen:true') }
        if (-not (Test-Path $options)) { Set-Content -Path $options -Value $lines -Encoding ASCII }
      }
      $log = Join-Path $gameDir 'logs\latest.log'
      $shots = Join-Path $gameDir 'screenshots'
      $runs = Join-Path $data 'logs\runs'
      $runsBefore = @(Get-ChildItem -Directory $runs -ErrorAction SilentlyContinue | ForEach-Object Name)
      $started = Get-Date; $playStarted = $started
      $launcherLogFrom = (Log-Text (Join-Path $data 'logs\launcher.log')).Length
      Click $ready
      $game = Wait-For { Game-Window } 300
      # What the launcher kept of the game's output (versions before 1.7 write no
      # latest.log; their output is only there).
      $runLog = Get-ChildItem -Directory $runs -ErrorAction SilentlyContinue |
        Where-Object { $runsBefore -notcontains $_.Name } | Select-Object -First 1 |
        ForEach-Object { Join-Path $_.FullName 'output.log' }
      $text = { (Log-Text $log) + "`n" + $(if ($runLog) { Log-Text $runLog } else { '' }) }
      if (-not $game) { $result.launch = 'failed'; $result.note = 'no game window in 5 minutes'; throw 'no game window' }
      $null = $game.Process.Handle  # keeps the exit code readable
      $up = Wait-For { $t = & $text; foreach ($m in $readyMarkers) { if ($t.Contains($m)) { return $true } } } 420
      if ($game.Process.HasExited) { $result.launch = 'failed'; $result.note = 'the game closed while loading'; throw 'exited' }
      if (-not $up) { Say "$label : no loading marker in the log; going on after the wait" }
      # The loading overlay fades out after the atlases; modded games load on for longer.
      Start-Sleep -Seconds $(if ($modded) { 30 } else { 15 })
      if ($game.Process.HasExited) { $result.launch = 'failed'; $result.note = 'the game closed after loading'; throw 'exited' }
      # A loading window of a loader may have given way to the game's own.
      # (the process object stays the first one: it holds the handle for the exit code).
      $again = Game-Window
      if ($again -and $again.Process.Id -eq $game.Process.Id) { $game.Hwnd = $again.Hwnd; $game.Title = $again.Title }
      $result.launchSeconds = [int]((Get-Date) - $started).TotalSeconds
      # The game that runs is the one asked for: the window title names its version
      # ("Minecraft 26.1"), and newer versions also log it through SDL.
      $running = if ((& $text) -match 'App version: (\S+)') { $Matches[1] }
        elseif ($game.Title -match '^Minecraft\*? (\d\S*)') { $Matches[1] } else { '' }
      # A special version is not checked by name: its title may name the release it
      # came from (2.0) or only part of its id.
      if (-not $isSpecial -and $running -and $running -ne $version) {
        $result.launch = 'wrong version'; $result.note = "the game started as $running"
        throw 'wrong version'
      }
      $result.launch = 'ok'
      Say "$label : title screen after $($result.launchSeconds) s ($($game.Title))"
      $hwnd = $game.Hwnd
      $layout = Gui-Layout $hwnd
      Say "$label : window $($layout.Width)x$($layout.Height), GUI scale $($layout.Scale)"
      Game-Shot $hwnd $folder 'start'
      if ($isSpecial) {
        # A version of the launcher's own list is checked to its title screen: its game
        # files came from the launcher's server, and some of these games have menus of
        # their own (the oldest none), so no world is made.
        Start-Sleep -Seconds 5
        Game-Shot $hwnd $folder 'title'
        $result.world = 'not checked'
      } else {
        # From 1.19.4 the first start shows the accessibility welcome screen. Its Continue
        # button is alone in the 33-high footer. Escape leaves it too if the click missed,
        # and does nothing on a title screen.
        if ((Compare-Version $version '1.19.4') -ge 0) {
          Game-Click $hwnd ($layout.GuiWidth / 2) ($layout.GuiHeight - 17)
          Start-Sleep -Seconds 3
        }
        Game-Key $hwnd 'Escape'
        Start-Sleep -Seconds 3
        Game-Shot $hwnd $folder 'title'

        # --- Singleplayer -> Create New World --------------------------------------------------
        $started = Get-Date
        # 1.4.6 to 1.6.x log the server's "<nick>[/127.0.0.1:0] logged in with entity id".
        $joined = "$([regex]::Escape($Nick)) joined the game|$([regex]::Escape($Nick))\[[^\]]*\] logged in with entity id"
        # Before 1.3 singleplayer had no server inside, and up to 1.4.5 the server inside
        # logs nowhere: there the new world's files show the world was made (any of
        # them: 1.2.4 got into its world in the test without a level.dat to be seen).
        $oldSingleplayer = (Compare-Version $version '1.4.6') -lt 0
        $inWorld = {
          if ((& $text) -match $joined) { return $true }
          if ($oldSingleplayer -and (Get-ChildItem (Join-Path $gameDir 'saves') -Recurse -File -ErrorAction SilentlyContinue |
              Where-Object { $_.LastWriteTime -gt $started })) { return $true }
        }
        # Since 1.19.3 a loader's world (Forge 1.19.3, NeoForge 1.20.5) may be made with
        # experimental features, and Create first asks to confirm them: Yes is the left of
        # two 150-wide buttons at height/6 + 96. Where the world is already being made, or
        # the question is not asked, that spot is a harmless one of Create New World.
        $confirms = (Compare-Version $version '1.19.3') -ge 0
        function Confirm-Experiments {
          if ($confirms -and -not (Wait-For $inWorld 5)) {
            Game-Click $hwnd ([math]::Floor($layout.GuiWidth / 2) - 155 + 75) ([math]::Floor($layout.GuiHeight / 6) + 96 + 10)
          }
        }
        # Singleplayer: the first button under the logo, 200x20 at height/4 + 48, in
        # every version.
        Game-Click $hwnd ($layout.GuiWidth / 2) ([math]::Floor($layout.GuiHeight / 4) + 48 + 10)
        Start-Sleep -Seconds 5
        Game-Shot $hwnd $folder 'worlds'
        # The world list's Create New World: the right of two 150-wide buttons at
        # height - 52. Newer versions open Create New World at once for an empty list,
        # and that spot is empty there.
        # Old versions (1.6.4 and before) may let a click pass, so it is given three
        # times (eight before 1.4.6): after one that worked, the next land on an empty
        # spot of Create New World.
        foreach ($i in 1..$(if ((Compare-Version $version '1.4.6') -lt 0) { 8 } else { 3 })) {
          Game-Click $hwnd ([math]::Floor($layout.GuiWidth / 2) + 4 + 75) ($layout.GuiHeight - 52 + 10)
          Start-Sleep -Milliseconds 1500
        }
        Start-Sleep -Seconds 2
        Game-Shot $hwnd $folder 'create-world'
        # Create New World: the left of two 150-wide buttons at the bottom (height - 28
        # in older versions, the 33-high footer in newer ones; the same spot).
        $createX = [math]::Floor($layout.GuiWidth / 2) - 155 + 75
        Game-Click $hwnd $createX ($layout.GuiHeight - 17)
        # Again for the same reason; a world being made does not mind the others.
        foreach ($i in 1..2) { if (-not (Wait-For $inWorld 3)) { Game-Click $hwnd $createX ($layout.GuiHeight - 17) } }
        Confirm-Experiments
        $entered = Wait-For $inWorld 60
        if (-not $entered -and (& $text) -match 'Starting integrated minecraft server') {
          # The world is being made: only slower than a minute.
          $entered = Wait-For $inWorld 240
        }
        if (-not $entered) {
          # The keyboard way, as a player without a mouse: back to the title screen, Tab to
          # Singleplayer, Enter; Enter again creates the world.
          Say "$label : no world after the clicks; the keyboard way"
          Game-Shot $hwnd $folder 'after-clicks'
          foreach ($i in 1..3) { Game-Key $hwnd 'Escape'; Start-Sleep -Seconds 1 }
          Game-Key $hwnd 'Tab'
          Game-Key $hwnd 'Enter'
          Start-Sleep -Seconds 5
          Game-Shot $hwnd $folder 'create-world-keys'
          Game-Key $hwnd 'Enter'
          Confirm-Experiments
          $entered = Wait-For $inWorld 240
          if ($entered) { $result.note = 'world entered with the keyboard' }
        }
        if (-not $entered) {
          $result.world = 'failed'; $result.note = 'did not get into a world'
          Game-Shot $hwnd $folder 'no-world'
          # Where the game saved, for an old version whose world is checked by its files:
          # the installation's saves, any new world file in the test's folders, and how
          # many test worlds the default game folder holds (names of other worlds there
          # are not listed).
          if ($oldSingleplayer) {
            $saved = @('installation saves:')
            $saved += @(Get-ChildItem (Join-Path $gameDir 'saves') -Recurse -File -ErrorAction SilentlyContinue |
              ForEach-Object { '{0:u} {1,10} {2}' -f $_.LastWriteTime.ToUniversalTime(), $_.Length, $_.FullName.Substring($gameDir.Length) })
            $saved += 'new world files in the test folder:'
            $saved += @(Get-ChildItem $root, $cache -Recurse -File -Include level.dat, session.lock -ErrorAction SilentlyContinue |
              Where-Object { $_.LastWriteTime -gt $started } | ForEach-Object { $_.FullName.Substring($env:LOCALAPPDATA.Length) })
            $default = Join-Path $env:APPDATA '.minecraft\saves'
            $tests = @(Get-ChildItem $default -Directory -ErrorAction SilentlyContinue | Where-Object { $_.Name -match '^New World-*$' })
            $saved += "default game folder: $($tests.Count) test worlds, $(@($tests | Where-Object { $_.LastWriteTime -gt $started }).Count) changed during this item"
            $changed = @('options.txt', 'saves', 'resources', 'screenshots', 'stats', 'lastlogin') | Where-Object {
              $entry = Get-Item (Join-Path $env:APPDATA ".minecraft\$_") -ErrorAction SilentlyContinue
              $entry -and $entry.LastWriteTime -gt $playStarted }
            $saved += "default game folder entries changed since the game started: $($changed -join ', ')"
            Set-Content (Join-Path $folder 'saves.txt') -Value $saved -Encoding UTF8
          }
        } else {
          $result.world = 'ok'
          if ((& $text) -match 'Starting integrated minecraft server version (\S+)' -and $Matches[1] -ne $version) {
            $result.world = 'wrong version'; $result.note = "the world's server is $($Matches[1])"
          }
          $result.worldSeconds = [int]((Get-Date) - $started).TotalSeconds
          Say "$label : in the world after $($result.worldSeconds) s"
          # A look around.
          Start-Sleep -Seconds 10
          Game-Shot $hwnd $folder 'world'
          # A world file of an old version is written before the world loads, and these
          # versions show a crash in the window and exit normally later (1.2.4 did): the
          # game's own F2 screenshot shows it is really in the world.
          if ($oldSingleplayer -and $result.world -eq 'ok' -and -not (Wait-For {
              Get-ChildItem $shots -Filter *.png -ErrorAction SilentlyContinue | Where-Object { $_.LastWriteTime -gt $started } } 10)) {
            $result.world = 'failed'; $result.note = 'no game screenshot in the world (a crash screen?)'
          }
        }
      }

      # --- Close the game window, as a player does ------------------------------------
      # What the window's close button sends (LWJGL 2, before 1.13, closes on it and lets
      # a bare WM_CLOSE pass); Alt+F4 as a player presses it if the window stays.
      [UserTest.Win]::PostMessage($hwnd, $WM_SYSCOMMAND, [IntPtr]$SC_CLOSE, [IntPtr]::Zero) | Out-Null
      if (-not $game.Process.WaitForExit(30000)) {
        Say "$label : still open after the close message; Alt+F4"
        if (Game-Focus $hwnd) {
          [UserTest.Win]::keybd_event(0x12, 0x38, 0, [IntPtr]::Zero)
          [UserTest.Win]::keybd_event(0x73, 0x3E, 0, [IntPtr]::Zero)
          Start-Sleep -Milliseconds 100
          [UserTest.Win]::keybd_event(0x73, 0x3E, 2, [IntPtr]::Zero)
          [UserTest.Win]::keybd_event(0x12, 0x38, 2, [IntPtr]::Zero)
        }
      }
      if (-not $game.Process.WaitForExit(90000)) {
        $result.exit = 'hung'; $result.note = "$($result.note); the game did not close in 2 minutes".Trim('; ')
        Stop-Process -Id $game.Process.Id -Force -ErrorAction SilentlyContinue
      } else {
        $result.exitCode = $game.Process.ExitCode
        # Before 1.6 the game's window listener prints "Someone is closing me!" and
        # exits with code 1 when the window is closed: that is the player's close. The
        # oldest games (Classic to Alpha 1.0) exit so without a word; the launcher's
        # tweaker prints its own line for them.
        Start-Sleep -Seconds 2
        $runText = if ($runLog) { Log-Text $runLog } else { '' }
        $windowClosed = $result.exitCode -eq 1 -and ($runText.Contains('Someone is closing me!') -or $runText.Contains('Launcher: the game window was closed'))
        $result.exit = if ($result.exitCode -eq 0 -or $windowClosed) { 'ok' } else { 'failed' }
      }
      $crashes = @(Get-ChildItem (Join-Path $gameDir 'crash-reports') -ErrorAction SilentlyContinue)
      if ($crashes) { $result.exit = 'crashed'; $result.note = "$($result.note); crash report".Trim('; ') }
      Start-Sleep -Seconds 3
    } catch {
      Say "$label : $($_.Exception.Message)"
      if ($result.note -eq '') { $result.note = $_.Exception.Message }
      $leftover = Game-Window
      if ($leftover) { Stop-Process -Id $leftover.Process.Id -Force -ErrorAction SilentlyContinue }
      if ($result.install -ne 'ok') {
        if ($script:main) { try { Dump-Launcher $folder } catch { } }
        Launcher-Log $folder
      }
    } finally {
      if ($null -ne $launcherLogFrom) { $result.skins = Skins-State $(if ($isSpecial) { 'vanilla' } else { $loader }) $launcherLogFrom $gameDir $runLog }
      $passed = Passed $result
      if ($result.skins -match '^not|^failed') { $result.note = "$($result.note); Launcher Skins $($result.skins)".Trim('; ') }
      elseif ($isSpecial -and $result.skins) { $result.note = "$($result.note); Launcher Skins added to a special version".Trim('; ') }
      if (Driver-Crashed $itemStarted) {
        $result.driver = 'crashed'
        $result.note = "$($result.note); the graphics driver crashed".Trim('; ')
        Close-DriverReports
      }
      if ($replayed[$item]) { $result.note = "$($result.note); played again after a graphics driver crash".Trim('; ') }
      if ($gameDir -and (Test-Path $log)) {
        $all = Log-Text $log
        $result.logErrors = ([regex]::Matches($all, '/(ERROR|FATAL)\]')).Count
        if (-not $passed) { [IO.File]::WriteAllText((Join-Path $folder 'latest.log'), (Scrub $all), (New-Object Text.UTF8Encoding $false)) }
      }
      if (-not $passed -and $runLog -and (Test-Path $runLog)) {
        [IO.File]::WriteAllText((Join-Path $folder 'output.log'), (Scrub (Log-Text $runLog)), (New-Object Text.UTF8Encoding $false))
      }
      if ($gameDir) {
        # The game's own screenshots only for a failure; the screen copies always.
        if (-not $passed) { Get-ChildItem $shots -Filter *.png -ErrorAction SilentlyContinue | Copy-Item -Destination $folder }
        Get-ChildItem (Join-Path $gameDir 'crash-reports') -ErrorAction SilentlyContinue | ForEach-Object {
          [IO.File]::WriteAllText((Join-Path $folder $_.Name), (Scrub ([IO.File]::ReadAllText($_.FullName))))
        }
      }
      Write-Host "::endgroup::"
      Say ("$label : install {0}, launch {1}, world {2}, exit {3} {4}" -f $result.install, $result.launch, $result.world, $result.exit, $result.note)
      # Kept after each one, so a cancelled run still has what it did.
      ConvertTo-Json -InputObject @($results) -Depth 4 | Set-Content -Encoding UTF8 (Join-Path $Out 'results.json')
    }
    if ($result.driver -and -not $passed -and -not $replayed[$item]) {
      Say "$label : the graphics driver crashed; played once more at the end"
      $replayed[$item] = $true
      $results.Remove($result)
      [void]$queue.Add($item)
    }
  }
} catch {
  Say "stopped: $($_.Exception.Message)"
  if ($script:main) { Dump-Launcher $Out }
  [void]$results.Add([ordered]@{ version = '(launcher)'; note = $_.Exception.Message })
} finally {
  ConvertTo-Json -InputObject @($results) -Depth 4 | Set-Content -Encoding UTF8 (Join-Path $Out 'results.json')
  Restore-Machine
}

$failed = @($results | Where-Object { -not (Passed $_) })
if ($failed) { exit 1 }

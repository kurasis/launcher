# Console window check (D-037) for the Windows package smoke test.
# Usage: powershell -File scripts/smoke/windows-console.ps1 [-Screenshot <png>]
#
# With the launcher running, opens More -> Diagnostics -> Open console through
# UI Automation (the way a user clicks), then reads the console window's
# accessibility tree: the toolbar and the journal lines the core wrote at start
# must be there. Prints what the window shows, saves a screenshot, and fails when
# the window is missing or empty.
param([string]$Screenshot = '')

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Drawing

$AE = [System.Windows.Automation.AutomationElement]
$Scope = [System.Windows.Automation.TreeScope]
$Button = [System.Windows.Automation.ControlType]::Button
$MenuItem = [System.Windows.Automation.ControlType]::MenuItem

function Wait-For([scriptblock]$Find, [int]$Seconds = 30) {
  $deadline = (Get-Date).AddSeconds($Seconds)
  do {
    $found = & $Find
    if ($found) { return $found }
    Start-Sleep -Milliseconds 500
  } while ((Get-Date) -lt $deadline)
  return $null
}

function Find-Window([string]$Name) {
  $condition = New-Object System.Windows.Automation.PropertyCondition($AE::NameProperty, $Name)
  return $AE::RootElement.FindFirst($Scope::Children, $condition)
}

# WebView2 reports CSS-transformed text ("DIAGNOSTICS"), so names are compared
# without regard to case.
function Find-Control($Within, $Type, [string]$Name) {
  $byType = New-Object System.Windows.Automation.PropertyCondition($AE::ControlTypeProperty, $Type)
  foreach ($candidate in $Within.FindAll($Scope::Descendants, $byType)) {
    if ($candidate.Current.Name -ieq $Name) { return $candidate }
  }
  return $null
}

function Find-Button($Within, [string]$Name) { return Find-Control $Within $Button $Name }

# The More button may be reported as a button or, since it opens a menu, as
# another control type; any focusable element of that name is taken.
function Find-Pressable($Within, [string]$Name) {
  foreach ($candidate in $Within.FindAll($Scope::Descendants, [System.Windows.Automation.Condition]::TrueCondition)) {
    if ($candidate.Current.Name -ieq $Name -and $candidate.Current.IsKeyboardFocusable) { return $candidate }
  }
  return $null
}

function Get-Texts($Within) {
  $all = $Within.FindAll($Scope::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
  return @($all | ForEach-Object { $_.Current.Name } | Where-Object { $_ })
}

Add-Type -Namespace Smoke -Name Mouse -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
[DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, System.IntPtr extra);
'@

# A button that opens a menu (aria-haspopup) offers no Invoke in WebView2: it is
# expanded through ExpandCollapse when offered, else clicked with the mouse at
# its centre, as a user does.
function Click($Element) {
  $pattern = $null
  if ($Element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
    $pattern.Invoke()
    return
  }
  if ($Element.TryGetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern, [ref]$pattern)) {
    try { $pattern.Expand(); return } catch { Write-Host "Expand failed: $($_.Exception.Message)" }
  }
  Click-Mouse $Element
}

function Click-Mouse($Element) {
  try { $main.SetFocus() } catch { }
  $r = $Element.Current.BoundingRectangle
  [Smoke.Mouse]::SetCursorPos([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2)) | Out-Null
  Start-Sleep -Milliseconds 100
  [Smoke.Mouse]::mouse_event(0x0002, 0, 0, 0, [System.IntPtr]::Zero)  # left down
  [Smoke.Mouse]::mouse_event(0x0004, 0, 0, 0, [System.IntPtr]::Zero)  # left up
}

function Save-Screenshot($Window) {
  if (-not $Screenshot -or -not $Window) { return }
  $r = $Window.Current.BoundingRectangle
  if ($r.Width -le 0 -or $r.Height -le 0) { return }
  $bitmap = New-Object System.Drawing.Bitmap([int]$r.Width, [int]$r.Height)
  $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
  $graphics.CopyFromScreen([int]$r.X, [int]$r.Y, 0, 0, $bitmap.Size)
  $bitmap.Save($Screenshot, [System.Drawing.Imaging.ImageFormat]::Png)
  Write-Host "Screenshot: $Screenshot"
}

$main = Wait-For { Find-Window 'Launcher' } 30
if (-not $main) { throw 'main window "Launcher" not found' }
# Diagnostics is in the More menu (D-067).
$more = Wait-For { Find-Pressable $main 'More' } 60
if (-not $more) {
  Write-Host 'Main window shows:'; Get-Texts $main | Select-Object -First 40 | Write-Host
  throw 'More menu not found in the main window'
}
$findDiagnostics = {
  $item = Find-Control $main $MenuItem 'Diagnostics'
  if (-not $item) { $item = Find-Pressable $main 'Diagnostics' }
  $item
}
Click $more
$tab = Wait-For $findDiagnostics 10
if (-not $tab) {
  Write-Host 'The More menu did not open; clicking it with the mouse'
  Click-Mouse $more
  $tab = Wait-For $findDiagnostics 20
}
if (-not $tab) {
  Write-Host 'Main window shows:'; Get-Texts $main | Select-Object -First 60 | Write-Host
  throw 'Diagnostics not found in the More menu'
}
Click $tab
$open = Wait-For { Find-Button $main 'Open console' } 30
if (-not $open) { throw '"Open console" button not found' }
Click $open

$console = Wait-For { Find-Window 'Launcher console' } 30
if (-not $console) { throw 'console window "Launcher console" did not open' }
Write-Host "Console window: $($console.Current.BoundingRectangle)"
# The core writes its start lines before any window exists; they must show.
$texts = Wait-For {
  $now = Get-Texts $console
  if ($now | Where-Object { $_ -like '*started on*' }) { return , $now }
} 30
Save-Screenshot $console
if (-not $texts) {
  $now = Get-Texts $console
  Write-Host "Console window shows $($now.Count) texts:"
  $now | Select-Object -First 40 | Write-Host
  throw 'the console window shows no journal lines'
}
Write-Host "Console window shows $($texts.Count) texts, including:"
$texts | Select-Object -First 30 | Write-Host
Write-Host 'Console window check passed'

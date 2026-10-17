# Console window check (D-037) for the Windows package smoke test.
# Usage: powershell -File scripts/smoke/windows-console.ps1 [-Screenshot <png>]
#
# With the launcher running, opens Diagnostics -> Open console through UI
# Automation (the way a user clicks), then reads the console window's
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

function Find-Button($Within, [string]$Name) {
  $byName = New-Object System.Windows.Automation.PropertyCondition($AE::NameProperty, $Name)
  $byType = New-Object System.Windows.Automation.PropertyCondition($AE::ControlTypeProperty, $Button)
  $both = New-Object System.Windows.Automation.AndCondition($byName, $byType)
  return $Within.FindFirst($Scope::Descendants, $both)
}

function Get-Texts($Within) {
  $all = $Within.FindAll($Scope::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
  return @($all | ForEach-Object { $_.Current.Name } | Where-Object { $_ })
}

function Click($Element) {
  $Element.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
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
$tab = Wait-For { Find-Button $main 'Diagnostics' } 60
if (-not $tab) {
  Write-Host 'Main window shows:'; Get-Texts $main | Select-Object -First 40 | Write-Host
  throw 'Diagnostics tab not found in the main window'
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

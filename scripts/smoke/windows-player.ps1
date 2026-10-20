# Player window check (D-067) for the Windows package smoke test.
# Usage: powershell -File scripts/smoke/windows-player.ps1 [-Screenshot <png>]
#
# With the launcher running on the feed, presses the first video of the feed
# through UI Automation, then reads the player window's accessibility tree: the
# embedded player in the play page must show something of its own, not only
# the page's "Open in browser" link. Prints what the window shows, saves a
# screenshot, and fails when the window is missing or the player is empty.
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

function Get-Texts($Within) {
  $all = $Within.FindAll($Scope::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
  return @($all | ForEach-Object { $_.Current.Name } | Where-Object { $_ })
}

# Control types and names, to see where the tree of the player's frame ends.
function Get-Tree($Within) {
  $all = $Within.FindAll($Scope::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
  return @($all | ForEach-Object { "$($_.Current.ControlType.ProgrammaticName) '$($_.Current.Name)'" })
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

# The tabs are buttons or tabs; whichever pattern the element offers presses it.
function Press($Element) {
  $pattern = $null
  if ($Element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
    $pattern.Invoke()
  } elseif ($Element.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern, [ref]$pattern)) {
    $pattern.Select()
  } else {
    throw "'$($Element.Current.Name)' cannot be pressed"
  }
}

$main = Wait-For { Find-Window 'Launcher' } 30
if (-not $main) { throw 'main window "Launcher" not found' }
# The console check left the main window on Diagnostics: back to the feed.
$feed = Wait-For {
  foreach ($candidate in $main.FindAll($Scope::Descendants, [System.Windows.Automation.Condition]::TrueCondition)) {
    if ($candidate.Current.Name -ieq 'Feed' -and $candidate.Current.IsKeyboardFocusable) { return $candidate }
  }
} 30
if (-not $feed) { throw 'Feed tab not found' }
Press $feed
# The feed's video cards are named "Watch “<title>” by <channel>"; streams,
# mixed in among them, "Watch <name> live: …" and are passed over.
$videoName = 'Watch ' + [char]0x201C + '*'
$video = Wait-For {
  $byType = New-Object System.Windows.Automation.PropertyCondition($AE::ControlTypeProperty, $Button)
  foreach ($candidate in $main.FindAll($Scope::Descendants, $byType)) {
    if ($candidate.Current.Name -like $videoName) { return $candidate }
  }
} 60
if (-not $video) {
  Write-Host 'Main window shows:'; Get-Texts $main | Select-Object -First 60 | Write-Host
  throw 'no video in the feed'
}
Press $video

$player = Wait-For { Find-Window 'Video' } 30
if (-not $player) { throw 'player window "Video" did not open' }
Write-Host "Player window: $($player.Current.BoundingRectangle)"
# The play page names only the window, its frame ("YouTube" or "Twitch", with
# WebView2's suffixes such as "- Web content (InPrivate)") and its link;
# anything else comes from the embedded player.
$own = '^(Video|YouTube|Twitch|Open in browser)( |$)'
$page = Wait-For { (Get-Texts $player) -contains 'Open in browser' } 30
if (-not $page) {
  Write-Host 'Player window tree:'
  Get-Tree $player | Select-Object -First 60 | Write-Host
  throw 'the play page did not load in the player window'
}
$texts = Wait-For {
  $theirs = @(Get-Texts $player | Where-Object { $_ -notmatch $own })
  if ($theirs.Count -gt 0) { return , $theirs }
} 45
Save-Screenshot $player
if (-not $texts) {
  Write-Host 'Player window tree:'
  Get-Tree $player | Select-Object -First 60 | Write-Host
  throw 'the player window shows nothing of the embedded player'
}
Write-Host "The embedded player shows $($texts.Count) texts, including:"
$texts | Select-Object -First 10 | Write-Host
Write-Host 'Player window check passed'

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

# Through the DevTools endpoint windows.ps1 opens: every page and frame the
# webviews hold, and for the play page and the players its frames, what they
# show and what they logged.
function Invoke-Cdp([string]$Url, [string[]]$Commands, [int]$ListenMs = 2000) {
  $socket = New-Object System.Net.WebSockets.ClientWebSocket
  $none = [System.Threading.CancellationToken]::None
  if (-not $socket.ConnectAsync([Uri]$Url, $none).Wait(5000)) { return @() }
  $id = 0
  foreach ($command in $Commands) {
    $id++
    $bytes = [System.Text.Encoding]::UTF8.GetBytes(('{"id":' + $id + ',' + $command.Substring(1)))
    $socket.SendAsync((New-Object System.ArraySegment[byte] (, $bytes)), 'Text', $true, $none).Wait(5000) | Out-Null
  }
  $messages = @()
  $buffer = New-Object byte[] 262144
  $text = New-Object System.Text.StringBuilder
  $deadline = (Get-Date).AddMilliseconds($ListenMs)
  while ($socket.State -eq 'Open') {
    $left = [int]($deadline - (Get-Date)).TotalMilliseconds
    if ($left -le 0) { break }
    $receive = $socket.ReceiveAsync((New-Object System.ArraySegment[byte] (, $buffer)), $none)
    if (-not $receive.Wait($left)) { break }
    [void]$text.Append([System.Text.Encoding]::UTF8.GetString($buffer, 0, $receive.Result.Count))
    if ($receive.Result.EndOfMessage) { $messages += $text.ToString(); [void]$text.Clear() }
  }
  try { $socket.Abort() } catch { }
  return $messages
}

function Show-Webviews {
  try {
    $targets = Invoke-RestMethod -Uri 'http://127.0.0.1:9222/json/list' -TimeoutSec 10
  } catch {
    Write-Host "DevTools endpoint: $($_.Exception.Message)"
    return
  }
  foreach ($target in $targets) {
    Write-Host "-- $($target.type): $($target.url)"
    if ($target.url -notmatch '^https://(feed\.lapi\.sh/play/|www\.youtube-nocookie\.com/|player\.twitch\.tv/)') { continue }
    $answers = Invoke-Cdp $target.webSocketDebuggerUrl @(
      '{"method":"Log.enable"}',
      '{"method":"Runtime.enable"}',
      '{"method":"Page.getFrameTree"}',
      '{"method":"Runtime.evaluate","params":{"expression":"document.readyState + \" | \" + document.title + \" | \" + (document.body ? document.body.innerText.slice(0, 300) : \"\")"}}'
    )
    foreach ($answer in $answers) {
      if ($answer -match 'executionContext') { continue }
      Write-Host ($answer.Substring(0, [Math]::Min(600, $answer.Length)))
    }
  }
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
# The feed's video cards are named "Watch “<title>” by <channel>".
$video = Wait-For {
  $byType = New-Object System.Windows.Automation.PropertyCondition($AE::ControlTypeProperty, $Button)
  foreach ($candidate in $main.FindAll($Scope::Descendants, $byType)) {
    if ($candidate.Current.Name -like 'Watch *') { return $candidate }
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
# The play page names only the window, its frame ("YouTube" or "Twitch") and
# its link; anything else comes from the embedded player.
$own = @('Video', 'YouTube', 'Twitch', 'Open in browser')
$page = Wait-For { (Get-Texts $player) -contains 'Open in browser' } 30
if (-not $page) {
  Write-Host 'Player window tree:'
  Get-Tree $player | Select-Object -First 60 | Write-Host
  Show-Webviews
  throw 'the play page did not load in the player window'
}
$texts = Wait-For {
  $theirs = @(Get-Texts $player | Where-Object { $own -notcontains $_ -and $_ -notlike '* - Web content' })
  if ($theirs.Count -gt 0) { return , $theirs }
} 45
Save-Screenshot $player
if (-not $texts) {
  Write-Host 'Player window tree:'
  Get-Tree $player | Select-Object -First 60 | Write-Host
  Show-Webviews
  throw 'the player window shows nothing of the embedded player'
}
Write-Host "The embedded player shows $($texts.Count) texts, including:"
$texts | Select-Object -First 10 | Write-Host
Write-Host 'Player window check passed'

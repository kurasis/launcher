# Package smoke test for the Windows NSIS installer.
# Usage: pwsh scripts/smoke/windows.ps1 <setup.exe>
#
# Installs silently for the current user, checks the per-user location and the
# uninstall entry, starts the launcher and waits until it holds the supervisor lock,
# opens the console window and checks what it shows (windows-console.ps1), plays
# the feed's first video in the player window (windows-player.ps1), then
# uninstalls silently and checks that DATA_ROOT is left in place.
param([Parameter(Mandatory = $true)][string]$Installer)

$ErrorActionPreference = 'Stop'
$waitSeconds = if ($env:SMOKE_WAIT_SECONDS) { [int]$env:SMOKE_WAIT_SECONDS } else { 20 }
$installDir = Join-Path $env:LOCALAPPDATA 'Launcher'
$exe = Join-Path $installDir 'launcher.exe'
$dataRoot = Join-Path $env:LOCALAPPDATA 'app.launcher'

function Step($text) { Write-Host "`n== $text" }

Step 'System'
(Get-CimInstance Win32_OperatingSystem | Select-Object Caption, Version, BuildNumber | Format-List | Out-String).Trim()
$webview = Get-ItemProperty -ErrorAction SilentlyContinue `
  'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
Write-Host "WebView2 runtime: $($webview.pv)"

Step "Install $(Split-Path $Installer -Leaf) silently (current user)"
$install = Start-Process -FilePath $Installer -ArgumentList '/S' -Wait -PassThru
if ($install.ExitCode -ne 0) { throw "installer exited with $($install.ExitCode)" }
if (-not (Test-Path $exe)) { throw "launcher not found at $exe" }
$entry = Get-ChildItem 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall' |
  Get-ItemProperty | Where-Object { $_.DisplayName -eq 'Launcher' }
if (-not $entry) { throw 'no per-user uninstall entry' }
Write-Host "Installed $($entry.DisplayVersion) to $installDir"

Step 'Start the installed launcher'
Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $dataRoot
$stdout = Join-Path $env:RUNNER_TEMP 'launcher-stdout.log'
$stderr = Join-Path $env:RUNNER_TEMP 'launcher-stderr.log'
# The DevTools endpoint lets windows-player.ps1 say what the player window
# loaded when it fails; it is opened only here, on the runner.
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9222'
$process = Start-Process -FilePath $exe -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
Start-Sleep -Seconds $waitSeconds
# A cold runner occasionally needs longer for the first start; allow up to 60 s.
$lock = Join-Path $dataRoot 'locks\supervisor.lock'
$deadline = (Get-Date).AddSeconds(60 - $waitSeconds)
while (-not (Test-Path $lock) -and -not $process.HasExited -and (Get-Date) -lt $deadline) {
  Start-Sleep -Seconds 1
}
if (-not (Test-Path $lock)) {
  # Show what the launcher did instead, so a failure can be diagnosed from the log.
  Write-Host "exited: $($process.HasExited)"
  Get-Process -ErrorAction SilentlyContinue launcher | Format-Table Id, Path, Responding | Out-String | Write-Host
  foreach ($base in $env:LOCALAPPDATA, $env:APPDATA) {
    Get-ChildItem -Directory $base | Where-Object Name -Like '*launcher*' | ForEach-Object {
      Write-Host $_.FullName
      Get-ChildItem -Recurse -Depth 1 -Name $_.FullName | Write-Host
    }
  }
  Write-Host '-- stdout'; Get-Content -ErrorAction SilentlyContinue $stdout | Write-Host
  Write-Host '-- stderr'; Get-Content -ErrorAction SilentlyContinue $stderr | Write-Host
}
if ($process.HasExited) { throw "launcher exited early with $($process.ExitCode)" }
if (-not (Test-Path (Join-Path $dataRoot 'locks\supervisor.lock'))) { throw 'DATA_ROOT not created' }
Write-Host "Running after ${waitSeconds}s, DATA_ROOT created"

Step 'Console window shows the journal'
$shot = Join-Path $env:RUNNER_TEMP 'console-window.png'
& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'windows-console.ps1') -Screenshot $shot
$consoleExit = $LASTEXITCODE
if ($consoleExit -ne 0) {
  Write-Host '-- stderr'; Get-Content -ErrorAction SilentlyContinue $stderr | Select-Object -Last 40 | Write-Host
  Stop-Process -Id $process.Id -Force
  throw "console window check failed ($consoleExit)"
}

Step 'Player window shows the embedded player'
$shot = Join-Path $env:RUNNER_TEMP 'player-window.png'
& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'windows-player.ps1') -Screenshot $shot
$playerExit = $LASTEXITCODE
if ($playerExit -ne 0) {
  Write-Host '-- stderr'; Get-Content -ErrorAction SilentlyContinue $stderr | Select-Object -Last 40 | Write-Host
  Stop-Process -Id $process.Id -Force
  throw "player window check failed ($playerExit)"
}
Stop-Process -Id $process.Id -Force
$process.WaitForExit()

Step 'Uninstall silently keeps user data'
$sentinelDir = Join-Path $dataRoot 'instances\smoke'
New-Item -ItemType Directory -Force $sentinelDir | Out-Null
Set-Content -Path (Join-Path $sentinelDir 'sentinel.txt') -Value 'kept'
$uninstaller = Join-Path $installDir 'uninstall.exe'
# _?= runs the uninstaller in place so that -Wait covers the whole removal.
$uninstall = Start-Process -FilePath $uninstaller -ArgumentList '/S', "_?=$installDir" -Wait -PassThru
if ($uninstall.ExitCode -ne 0) { throw "uninstaller exited with $($uninstall.ExitCode)" }
if (Test-Path $exe) { throw 'launcher still installed' }
if ((Get-Content (Join-Path $sentinelDir 'sentinel.txt')) -ne 'kept') { throw 'user data removed' }
Write-Host "Uninstall left $dataRoot in place"

Step 'Windows package smoke test passed'

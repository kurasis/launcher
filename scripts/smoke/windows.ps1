# Package smoke test for the Windows NSIS installer.
# Usage: pwsh scripts/smoke/windows.ps1 <setup.exe>
#
# Installs silently for the current user, checks the per-user location and the
# uninstall entry, starts the launcher and waits until it holds the supervisor lock,
# then uninstalls silently and checks that DATA_ROOT is left in place.
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
$process = Start-Process -FilePath $exe -PassThru
Start-Sleep -Seconds $waitSeconds
if ($process.HasExited) { throw "launcher exited early with $($process.ExitCode)" }
if (-not (Test-Path (Join-Path $dataRoot 'locks\supervisor.lock'))) { throw 'DATA_ROOT not created' }
Write-Host "Running after ${waitSeconds}s, DATA_ROOT created"
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

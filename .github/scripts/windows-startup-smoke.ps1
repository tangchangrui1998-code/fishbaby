$ErrorActionPreference = 'Stop'
$installPath = Join-Path $env:RUNNER_TEMP 'fishbaby-installed'
$diagnostics = Join-Path $env:RUNNER_TEMP 'fishbaby-diagnostics'
New-Item -ItemType Directory -Force $diagnostics | Out-Null
$logPath = Join-Path $diagnostics 'startup.log'
$installer = Get-ChildItem 'src-tauri/target/release/bundle/nsis/*-setup.exe' | Select-Object -First 1
if (!$installer) { throw 'Installer is missing' }
$install = Start-Process -FilePath $installer.FullName -ArgumentList @('/S', "/D=$installPath") -Wait -PassThru
if ($install.ExitCode -ne 0) { throw "Installer failed: $($install.ExitCode)" }
$exe = Join-Path $installPath 'fishbaby.exe'
if (!(Test-Path $exe)) { throw "Installed executable not found: $exe" }
Get-Item $exe | Select-Object FullName, Length, VersionInfo | Format-List | Out-String | Set-Content (Join-Path $diagnostics 'installed-file.txt')
$info = [System.Diagnostics.ProcessStartInfo]::new()
$info.FileName = $exe
$info.WorkingDirectory = $installPath
$info.UseShellExecute = $false
$info.Environment['FISHBABY_STARTUP_LOG'] = $logPath
$app = [System.Diagnostics.Process]::Start($info)
try {
  $deadline = (Get-Date).AddSeconds(60)
  $ready = $false
  while ((Get-Date) -lt $deadline) {
    $app.Refresh()
    if ($app.HasExited) { throw "Application exited during startup: $($app.ExitCode)" }
    if (Test-Path $logPath) {
      $log = Get-Content $logPath -Raw
      if ($log -match 'frontend-ready main') { $ready = $true; break }
    }
    Start-Sleep -Milliseconds 300
  }
  if (!$ready) { throw 'Installed app did not render the bookshelf within 60 seconds' }
  Write-Output 'Installed Windows application rendered the bookshelf successfully.'
} finally {
  $fallback = Join-Path $env:LOCALAPPDATA 'com.fishbaby.reader/logs/startup.log'
  if (Test-Path $fallback) { Copy-Item $fallback (Join-Path $diagnostics 'startup-default.log') }
  Get-ChildItem $diagnostics -Filter '*.log' | ForEach-Object { Write-Output $_.Name; Get-Content $_.FullName }
  $app.Refresh()
  if (!$app.HasExited) {
    $app | Select-Object Id,ProcessName,MainWindowTitle,Responding,Path | Format-List | Out-String | Set-Content (Join-Path $diagnostics 'process-state.txt')
    Stop-Process -Id $app.Id -Force
  }
}

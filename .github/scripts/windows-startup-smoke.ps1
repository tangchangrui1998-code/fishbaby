$ErrorActionPreference = 'Stop'
$installPath = Join-Path $env:RUNNER_TEMP 'fishbaby-installed'
$env:FISHBABY_STARTUP_LOG = Join-Path $env:RUNNER_TEMP 'fishbaby-startup.log'
$installer = Get-ChildItem 'src-tauri/target/release/bundle/nsis/*-setup.exe' | Select-Object -First 1
if (!$installer) { throw 'Installer is missing' }
$install = Start-Process -FilePath $installer.FullName -ArgumentList @('/S', "/D=$installPath") -Wait -PassThru
if ($install.ExitCode -ne 0) { throw "Installer failed: $($install.ExitCode)" }
$exe = Join-Path $installPath 'fishbaby.exe'
if (!(Test-Path $exe)) { throw "Installed executable not found: $exe" }
$app = Start-Process -FilePath $exe -PassThru
try {
  $deadline = (Get-Date).AddSeconds(60)
  $ready = $false
  while ((Get-Date) -lt $deadline) {
    $app.Refresh()
    if ($app.HasExited) { throw "Application exited during startup: $($app.ExitCode)" }
    if (Test-Path $env:FISHBABY_STARTUP_LOG) {
      $log = Get-Content $env:FISHBABY_STARTUP_LOG -Raw
      if ($log -match 'frontend-ready main') { $ready = $true; break }
    }
    Start-Sleep -Milliseconds 300
  }
  if (!$ready) { throw 'Installed app did not render the bookshelf within 60 seconds' }
  Write-Output 'Installed Windows application rendered the bookshelf successfully.'
} finally {
  if (Test-Path $env:FISHBABY_STARTUP_LOG) { Get-Content $env:FISHBABY_STARTUP_LOG }
  $app.Refresh()
  if (!$app.HasExited) { Stop-Process -Id $app.Id -Force }
}

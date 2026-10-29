$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true') { throw 'This check uses a disposable GitHub runner profile.' }
$installPath = Join-Path $env:RUNNER_TEMP 'fishbaby-installed'
$diagnostics = Join-Path $env:RUNNER_TEMP 'fishbaby-diagnostics'
New-Item -ItemType Directory -Force $diagnostics | Out-Null
$logPath = Join-Path $diagnostics 'startup.log'
$readerLogDirectory = Join-Path $env:LOCALAPPDATA 'com.fishbaby.reader/logs'
$readerLogPath = Join-Path $readerLogDirectory 'reader.jsonl'
$version = (Get-Content 'package.json' -Raw | ConvertFrom-Json).version
$installers = @(Get-ChildItem "src-tauri/target/release/bundle/nsis/*_${version}_x64-setup.exe")
if ($installers.Count -ne 1) { throw "Expected exactly one installer for $version, found $($installers.Count)" }
$installer = $installers[0]
Write-Output "Testing installer: $($installer.Name)"
$install = Start-Process -FilePath $installer.FullName -ArgumentList @('/S', "/D=$installPath") -Wait -PassThru
if ($install.ExitCode -ne 0) { throw "Installer failed: $($install.ExitCode)" }
$exe = Join-Path $installPath 'fishbaby.exe'
if (!(Test-Path $exe)) { throw "Installed executable not found: $exe" }
Get-Item $exe | Select-Object FullName, Length, VersionInfo | Format-List | Out-String | Set-Content (Join-Path $diagnostics 'installed-file.txt')
$installedVersion = (Get-Item $exe).VersionInfo.ProductVersion
if ($installedVersion -ne $version) { throw "Installed version $installedVersion does not match requested $version" }
$libraryDirectory = Join-Path $env:APPDATA 'com.fishbaby.reader'
New-Item -ItemType Directory -Force $libraryDirectory | Out-Null
$libraryFile = Join-Path $libraryDirectory 'library.json'
$priorLibrary = if (Test-Path $libraryFile) { [IO.File]::ReadAllBytes($libraryFile) } else { $null }
$bookPath = Join-Path $env:RUNNER_TEMP 'fishbaby-smoke.txt'
$fixture = (1..500 | ForEach-Object { "Chapter $_`nA quiet morning and a saved reading position.`n" }) -join ''
[IO.File]::WriteAllText($bookPath, $fixture, [Text.UTF8Encoding]::new($false))
$library = @{ books = @(@{ id='windows-smoke'; title='Windows Smoke Book'; path=$bookPath; offset=0; length=$fixture.Length; fingerprint=(Get-FileHash $bookPath -Algorithm SHA256).Hash.ToLower(); lastRead=0; anchor=''; available=$true }); activeId=$null; settings=@{ width=480; height=240 } }
$library | ConvertTo-Json -Depth 6 | Set-Content $libraryFile -Encoding utf8NoBOM
$info = [System.Diagnostics.ProcessStartInfo]::new()
$info.FileName = $exe
$info.WorkingDirectory = $installPath
$info.UseShellExecute = $false
$info.Environment['FISHBABY_STARTUP_LOG'] = $logPath
$info.Environment['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS'] = '--force-renderer-accessibility'
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
  & "$env:WINDIR/System32/WindowsPowerShell/v1.0/powershell.exe" -NoProfile -MTA -File "$PSScriptRoot/windows-reader-check.ps1" -AppProcessId $app.Id -LogPath $logPath -LibraryPath $libraryFile -DiagnosticsPath $diagnostics -ReaderLogPath $readerLogPath
  if ($LASTEXITCODE -ne 0) { throw 'Installed reader workflow verification failed' }
} finally {
  if (Test-Path $readerLogDirectory) { Get-ChildItem $readerLogDirectory -Filter 'reader*.jsonl' | Copy-Item -Destination $diagnostics }
  $fallback = Join-Path $env:LOCALAPPDATA 'com.fishbaby.reader/logs/startup.log'
  if (Test-Path $fallback) { Copy-Item $fallback (Join-Path $diagnostics 'startup-default.log') }
  Get-ChildItem $diagnostics -Filter '*.log' | ForEach-Object { Write-Output $_.Name; Get-Content $_.FullName }
  $app.Refresh()
  if (!$app.HasExited) {
    $app | Select-Object Id,ProcessName,MainWindowTitle,Responding,Path | Format-List | Out-String | Set-Content (Join-Path $diagnostics 'process-state.txt')
    Stop-Process -Id $app.Id -Force
  }
  if ($null -ne $priorLibrary) { [IO.File]::WriteAllBytes($libraryFile, $priorLibrary) }
  elseif (Test-Path $libraryFile) { Remove-Item $libraryFile }
}

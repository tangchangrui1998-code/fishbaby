param([int]$AppProcessId, [string]$LogPath, [string]$LibraryPath, [string]$DiagnosticsPath, [string]$ReaderLogPath)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class ReaderCheckNative {
  [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
  public struct Dimensions { public int Width, Height; public Dimensions(int w, int h) { Width = w; Height = h; } }
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out Rect rect);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out Rect rect);
  [DllImport("user32.dll", EntryPoint="GetWindowLongW")] public static extern int GetWindowLong(IntPtr h, int index);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint flags);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint x, uint y, uint data, UIntPtr extra);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr CreateWindowEx(uint ex, string cls, string title, uint style, int x, int y, int w, int h, IntPtr parent, IntPtr menu, IntPtr instance, IntPtr param);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int height, uint flags);
  [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint relation);
  [DllImport("user32.dll")] public static extern bool DestroyWindow(IntPtr h);
  public static void Click(int x, int y) {
    SetCursorPos(x, y);
    mouse_event(2, 0, 0, 0, UIntPtr.Zero);
    mouse_event(4, 0, 0, 0, UIntPtr.Zero);
  }
  public static bool Above(IntPtr top, IntPtr bottom) {
    for (int i=0; i<512; i++) {
      bottom = GetWindow(bottom, 3);
      if (bottom == IntPtr.Zero) return false;
      if (bottom == top) return true;
    }
    return false;
  }
  public static Dimensions ClientSize(IntPtr h) {
    Rect rect;
    if (!GetClientRect(h, out rect)) throw new Exception("Cannot read client size");
    return new Dimensions(rect.Right - rect.Left, rect.Bottom - rect.Top);
  }
  public static bool ResizeClient(IntPtr h, int width, int height) {
    Rect client, outer;
    if (!GetClientRect(h, out client) || !GetWindowRect(h, out outer)) return false;
    return SetWindowPos(h, IntPtr.Zero, 0, 0,
      width + outer.Right - outer.Left - client.Right + client.Left,
      height + outer.Bottom - outer.Top - client.Bottom + client.Top, 0x16);
  }
}
'@
function Wait-For([scriptblock]$Check, [string]$Message) {
  $deadline = (Get-Date).AddSeconds(25)
  while ((Get-Date) -lt $deadline) {
    [System.Windows.Forms.Application]::DoEvents()
    try {
      $result = & $Check
      if ($result) { return $result }
    } catch { $lastCheckError = $_ }
    Start-Sleep -Milliseconds 100
  }
  throw "$Message. Last check error: $lastCheckError"
}
function Find-Button($Window, [string]$Name) {
  $condition = [System.Windows.Automation.AndCondition]::new([System.Windows.Automation.Condition[]]@(
    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty, $Name),
    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Button)
  ))
  $button = $Window.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $condition)
  if ($button -and !$button.Current.IsOffscreen -and $button.Current.IsEnabled) { return $button }
}
function Wait-State([string]$Name) {
  Wait-For {
    $latest = Get-Content $LogPath | Where-Object { $_ -match 'reader-view ' } | Select-Object -Last 1
    $latest -match "reader-view \d+ $Name`$"
  } "Reader did not enter $Name" | Out-Null
}
function Reader-Events {
  if (!(Test-Path $ReaderLogPath)) { return @() }
  @(Get-Content $ReaderLogPath -Tail 100 | ForEach-Object {
    try { $event = $_ | ConvertFrom-Json; if ($event.pid -eq $AppProcessId) { $event } } catch {}
  })
}
function Wait-FrontendState([string]$Name) {
  Wait-For {
    $event = Reader-Events | Where-Object { $_.event -eq 'frontend-view' } | Select-Object -Last 1
    $event.details.state -eq $Name -and $event.details.contentVisible -eq ($Name -ne 'hidden')
  } "Frontend did not apply $Name with the expected CSS visibility" | Out-Null
}
$processCondition = [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty, $AppProcessId)
$readLabel = -join [char[]]@(0x9605,0x8BFB)
$startLabel = -join [char[]]@(0x5F00,0x59CB,0x9605,0x8BFB)
$pauseLabel = -join [char[]]@(0x6682,0x505C,0x9605,0x8BFB)
$themeLabel = -join [char[]]@(0x5207,0x6362,0x5230,0x6DF1,0x8272,0x4E3B,0x9898)
$readerTitle = -join [char[]]@(0x9690,0x9605,0x20,0xB7,0x20,0x9605,0x8BFB,0x533A,0x57DF)
$cover = [IntPtr]::Zero
$coverForm = $null
try {
  $main = Wait-For {
    $windows = [System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, $processCondition)
    foreach ($window in $windows) { if (!$window.Current.IsOffscreen) { return $window } }
  } 'Bookshelf window not accessible'
  $read = Wait-For { Find-Button $main $readLabel } 'Read button not accessible'
  $read.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
  $reader = Wait-For {
    $windows = [System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, $processCondition)
    foreach ($window in $windows) { if ($window.Current.Name -eq $readerTitle) { return $window } }
  } 'Reader window not created'
  $start = Wait-For { Find-Button $reader $startLabel } 'First reader preview stayed hidden (without Adjust Reading Area)'
  $readerHandle = [ReaderCheckNative]::GetAncestor([IntPtr]$reader.Current.NativeWindowHandle, 2)
  if (![ReaderCheckNative]::IsWindowVisible($readerHandle)) { throw 'Reader native window is not visible' }
  if (([ReaderCheckNative]::GetWindowLong($readerHandle, -20) -band 8) -ne 0) { throw 'Reader is still topmost' }
  Write-Output 'First read displayed a visible preview without adjusting the reading area.'
  $buttonRect = $start.Current.BoundingRectangle
  [ReaderCheckNative]::SetCursorPos([int]($buttonRect.X + $buttonRect.Width/2), [int]($buttonRect.Y + $buttonRect.Height/2)) | Out-Null
  $start.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
  Wait-State 'reading'
  Wait-FrontendState 'reading'
  if (([ReaderCheckNative]::GetWindowLong($readerHandle, -16) -band 0x40000) -ne 0) { throw 'Reading window remained manually resizable' }
  $initialSize = [ReaderCheckNative]::ClientSize($readerHandle)
  $initialSettings = (Get-Content $LibraryPath -Raw | ConvertFrom-Json).settings
  $bounds = $reader.Current.BoundingRectangle
  $bodyY = [int]($bounds.Y + $bounds.Height * 0.55)
  [ReaderCheckNative]::Click([int]($bounds.X + $bounds.Width * 0.75), $bodyY)
  Wait-For { (Get-Content $LibraryPath -Raw | ConvertFrom-Json).books[0].offset -gt 0 } 'Right content click did not turn forward' | Out-Null
  [ReaderCheckNative]::Click([int]($bounds.X + $bounds.Width * 0.25), $bodyY)
  Wait-For { (Get-Content $LibraryPath -Raw | ConvertFrom-Json).books[0].offset -eq 0 } 'Left content click did not return to the same page' | Out-Null
  Write-Output 'Reader left/right content clicks saved and restored the original position.'
  $coverForm = [System.Windows.Forms.Form]::new()
  $coverForm.Text = 'fishbaby smoke cover'
  $coverForm.TopMost = $false
  $coverForm.StartPosition = [System.Windows.Forms.FormStartPosition]::Manual
  $coverForm.FormBorderStyle = [System.Windows.Forms.FormBorderStyle]::None
  $coverForm.Bounds = [System.Drawing.Rectangle]::new([int]$bounds.X, [int]$bounds.Y, [int]$bounds.Width, [int]$bounds.Height)
  $coverForm.Show()
  $coverForm.BringToFront()
  $coverForm.Activate()
  $cover = $coverForm.Handle
  Wait-For { [ReaderCheckNative]::IsWindowVisible($cover) -and [ReaderCheckNative]::Above($cover, $readerHandle) } 'Covering test window is not above the reader' | Out-Null
  Write-Output "Cover root $cover is visible above reader root $readerHandle."
  Wait-State 'hidden'
  Wait-FrontendState 'hidden'
  [ReaderCheckNative]::SetCursorPos([int]($bounds.X + $bounds.Width/2), $bodyY) | Out-Null
  Start-Sleep -Milliseconds 400
  [System.Windows.Forms.Application]::DoEvents()
  if (![ReaderCheckNative]::Above($cover, $readerHandle)) { throw 'Hover raised the reader above the covering window' }
  Wait-State 'hidden'
  Write-Output 'A later normal window covered the reader; mouse movement did not reveal or raise it.'
  if (![ReaderCheckNative]::ResizeClient($readerHandle, $initialSize.Width * 2, $initialSize.Height * 2)) { throw 'Cannot simulate system-driven reader enlargement' }
  Wait-For {
    $size = [ReaderCheckNative]::ClientSize($readerHandle)
    [Math]::Abs($size.Width - $initialSize.Width) -le 1 -and [Math]::Abs($size.Height - $initialSize.Height) -le 1
  } 'Hidden reader did not restore its original dimensions' | Out-Null
  $settings = (Get-Content $LibraryPath -Raw | ConvertFrom-Json).settings
  if ($settings.width -ne $initialSettings.width -or $settings.height -ne $initialSettings.height) { throw 'A system resize polluted the saved reading dimensions' }
  if (![ReaderCheckNative]::Above($cover, $readerHandle)) { throw 'Repairing dimensions raised the reader over its cover' }
  Wait-For { Reader-Events | Where-Object { $_.event -eq 'geometry-restore' } } 'Reader did not record the geometry repair' | Out-Null
  Wait-State 'hidden'
  Write-Output 'Hidden reader recovered from an unexpected doubled size, preserved preferences and stayed below its cover.'
  $coverForm.Close()
  $coverForm.Dispose()
  $coverForm = $null
  $cover = [IntPtr]::Zero
  Wait-State 'reading'
  Write-Output 'Reader resumed hover only after the covering window was removed.'
  $theme = Wait-For { Find-Button $reader $themeLabel } 'Theme toggle not accessible'
  $theme.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
  Wait-For { (Get-Content $LibraryPath -Raw | ConvertFrom-Json).settings.theme -eq 'dark' } 'Theme did not change' | Out-Null
  $size = [ReaderCheckNative]::ClientSize($readerHandle)
  if ($size.Width -ne $initialSize.Width -or $size.Height -ne $initialSize.Height) { throw 'Theme change reset the reading dimensions' }
  $pause = Wait-For { Find-Button $reader $pauseLabel } 'Pause button not accessible'
  $pause.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
  Wait-State 'ready'
  $start = Wait-For { Find-Button $reader $startLabel } 'Preview did not become editable again'
  $targetWidth = $initialSize.Width + 100
  $targetHeight = $initialSize.Height + 60
  if (![ReaderCheckNative]::ResizeClient($readerHandle, $targetWidth, $targetHeight)) { throw 'Cannot resize the editable preview' }
  Wait-For {
    $saved = (Get-Content $LibraryPath -Raw | ConvertFrom-Json).settings
    $saved.width -gt $initialSettings.width -and $saved.height -gt $initialSettings.height
  } 'Manual preview dimensions were not saved' | Out-Null
  $savedSize = [ReaderCheckNative]::ClientSize($readerHandle)
  if ([Math]::Abs($savedSize.Width - $targetWidth) -gt 1 -or [Math]::Abs($savedSize.Height - $targetHeight) -gt 1) { throw 'Preview resizing was incorrectly locked' }
  $bounds = $reader.Current.BoundingRectangle
  $buttonRect = $start.Current.BoundingRectangle
  [ReaderCheckNative]::SetCursorPos([int]($buttonRect.X + $buttonRect.Width/2), [int]($buttonRect.Y + $buttonRect.Height/2)) | Out-Null
  $start.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
  Wait-State 'reading'
  for ($cycle = 0; $cycle -lt 5; $cycle++) {
    [ReaderCheckNative]::SetCursorPos([int]($bounds.X - 10), [int]($bounds.Y - 10)) | Out-Null
    Wait-State 'hidden'
    Wait-FrontendState 'hidden'
    [ReaderCheckNative]::SetCursorPos([int]($bounds.X + $bounds.Width/2), [int]($bounds.Y + $bounds.Height/2)) | Out-Null
    Wait-State 'reading'
    Wait-FrontendState 'reading'
  }
  $size = [ReaderCheckNative]::ClientSize($readerHandle)
  if ($size.Width -ne $savedSize.Width -or $size.Height -ne $savedSize.Height) { throw 'Hover cycles changed the manually chosen size' }
  Write-Output 'Theme sizing, editable preview and repeated native/frontend hover transitions passed.'
  'First preview, click paging, normal stacking, covered hover, hidden size recovery, theme sizing, editable preview, repeated hover and local diagnostics passed.' | Set-Content (Join-Path $DiagnosticsPath 'reader-workflow.txt')
} catch {
  $errorText = $_ | Out-String
  $errorText | Set-Content (Join-Path $DiagnosticsPath 'reader-workflow-error.txt')
  throw
} finally {
  if ($null -ne $coverForm) { $coverForm.Close(); $coverForm.Dispose() }
}

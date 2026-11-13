#Requires -Version 7.4
# Native quit smoke: starts one isolated GUI instance, closes its window through
# UI Automation and checks for an orderly exit. With -DuringScan, the window is
# closed while a scan job is running. Use a GUI build and an interactive session.
[CmdletBinding()]
param(
  [Parameter(Mandatory)][string]$Binary,
  [switch]$DuringScan,
  # Also read app.startup_status and app.product_info through a local DevTools port.
  [switch]$ProbeIpc,
  [int]$DebugPort = 9229,
  [switch]$KeepFixture
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$exe = (Resolve-Path -LiteralPath $Binary).Path
# Only one GUI instance may run; wait for another owner instead of stopping it.
while (Get-CimInstance Win32_Process -Filter "Name = 'diskunfold.exe' AND ThreadCount > 0") {
  Write-Host 'Another DiskUnfold instance is running; waiting 30 seconds.'
  Start-Sleep -Seconds 30
}
$root = Join-Path ([IO.Path]::GetTempPath()) ('diskunfold-quit-' + [guid]::NewGuid())
$profileDir = Join-Path $root 'profile'
$dataDir = Join-Path $profileDir 'DiskUnfold'
$scope = Join-Path $root 'scope'
New-Item -ItemType Directory -Path $profileDir, $scope | Out-Null
[IO.File]::WriteAllBytes((Join-Path $scope 'root.bin'), [byte[]]::new(11))
if ($DuringScan) {
  # Enough small files that the scan is still running when the close request arrives.
  $contents = [byte[]]::new(1)
  for ($group = 0; $group -lt 64; $group++) {
    $folder = Join-Path $scope "group-$group"
    [IO.Directory]::CreateDirectory($folder) | Out-Null
    for ($index = 0; $index -lt 800; $index++) {
      [IO.File]::WriteAllBytes((Join-Path $folder "file-$index.bin"), $contents)
    }
  }
}
$ae = [Windows.Automation.AutomationElement]
$tree = [Windows.Automation.TreeScope]::Descendants
function Find-UI($name, $type) {
  $condition = [Windows.Automation.PropertyCondition]::new($ae::NameProperty, $name)
  if ($type) {
    $condition = [Windows.Automation.AndCondition]::new($condition,
      [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, $type))
  }
  $window.FindFirst($tree, $condition)
}
function Wait-UI($label, [scriptblock]$probe, $seconds = 30) {
  $deadline = [DateTime]::UtcNow.AddSeconds($seconds)
  do {
    if ($process.HasExited) { throw "Application exited while waiting for $label" }
    try { $found = & $probe; if ($found) { return $found } }
    catch [Windows.Automation.ElementNotAvailableException] {}
    Start-Sleep -Milliseconds 50
  } while ([DateTime]::UtcNow -lt $deadline)
  throw "Timed out waiting for $label"
}
function Invoke-UI($item) {
  $item.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Get-DataFiles {
  if (Test-Path -LiteralPath $dataDir) {
    # Product files only; the WebView2 profile below webview\ has its own databases.
    Get-ChildItem -LiteralPath $dataDir -Force -File |
      ForEach-Object { '{0} {1}' -f $_.Name, $_.Length }
  }
}
$button = [Windows.Automation.ControlType]::Button
$edit = [Windows.Automation.ControlType]::Edit
# UI Automation names from user-env/i18n for the two shipped locales.
$names = @{
  en = @{ path = 'Windows folder path'; review = 'Review scope'; start = 'Start scan'; cancel = 'Cancel scan'; finished = 'Scan finished' }
  ko = @{ path = 'Windows 폴더 경로'; review = '스캔 범위 검토'; start = '스캔 시작'; cancel = '스캔 취소'; finished = '스캔 완료' }
}
$environment = @{ LOCALAPPDATA = $profileDir; DISKUNFOLD_DATA_DIR = $dataDir }
if ($ProbeIpc) { $environment.WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$DebugPort" }
$process = Start-Process -FilePath $exe -PassThru -Environment $environment -RedirectStandardError (Join-Path $root 'stderr.txt') -RedirectStandardOutput (Join-Path $root 'stdout.txt')
$null = $process.Handle  # keep the handle so ExitCode stays readable
$processId = $process.Id
$result = [ordered]@{ scenario = $(if ($DuringScan) { 'close-during-scan' } else { 'close-idle' }); pid = $processId }
try {
  $window = Wait-UI 'main window' {
    $process.Refresh()
    if ($process.MainWindowHandle -ne 0) { $ae::FromHandle($process.MainWindowHandle) }
  }
  $locale = Wait-UI 'first screen' {
    foreach ($code in $names.Keys) { if (Find-UI $names[$code].path $edit) { $code; break } }
  }
  $ui = $names[$locale]
  $result.startupLocale = $locale
  if ($ProbeIpc) {
    $probe = node (Join-Path $PSScriptRoot 'smoke-webview-ipc.mjs') $DebugPort app.startup_status app.product_info
    if ($LASTEXITCODE -ne 0) { throw 'The IPC probe failed' }
    $result.ipc = $probe | ConvertFrom-Json
  }
  $result.filesBeforeClose = @(Get-DataFiles)
  if ($DuringScan) {
    $field = Find-UI $ui.path $edit
    $field.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).SetValue($scope)
    $review = Wait-UI 'Review scope' { $b = Find-UI $ui.review $button; if ($b -and $b.Current.IsEnabled) { $b } }
    Invoke-UI $review
    $start = Wait-UI 'Start scan' { $b = Find-UI $ui.start $button; if ($b -and $b.Current.IsEnabled) { $b } }
    Invoke-UI $start
    $null = Wait-UI 'running scan (Cancel scan available)' {
      $b = Find-UI $ui.cancel $button; if ($b -and $b.Current.IsEnabled) { $b }
    }
    $result.scanFinishedBeforeClose = [bool](Find-UI $ui.finished ([Windows.Automation.ControlType]::Text))
  }
  $window.GetCurrentPattern([Windows.Automation.WindowPattern]::Pattern).Close()
  $closedAt = [DateTime]::UtcNow
  $result.exitedWithinTenSeconds = $process.WaitForExit(10000)
  if (-not $result.exitedWithinTenSeconds) { throw 'The process did not exit after the window close request' }
  $result.exitSeconds = [Math]::Round(([DateTime]::UtcNow - $closedAt).TotalSeconds, 2)
} catch {
  Write-Warning "Quit smoke failed. Fixture and logs: $root"
  if (-not $process.HasExited) { Stop-Process -Id $processId }
  throw
}
$result.exitCode = $process.ExitCode
$result.processGone = -not (Get-Process -Id $processId -ErrorAction SilentlyContinue)
$result.noDiskUnfoldProcess = -not (Get-CimInstance Win32_Process -Filter "Name = 'diskunfold.exe' AND ThreadCount > 0")
# WebView2 helpers of this profile must disappear with the host.
$deadline = [DateTime]::UtcNow.AddSeconds(10)
do {
  $helpers = @(Get-CimInstance Win32_Process -Filter "Name = 'msedgewebview2.exe'" |
    Where-Object { $_.CommandLine -like "*$profileDir*" })
  if (-not $helpers) { break }
  Start-Sleep -Milliseconds 200
} while ([DateTime]::UtcNow -lt $deadline)
$result.webviewHelpersLeft = $helpers.Count
# Releasing the instance lock means an exclusive open of the sentinel succeeds.
$sentinel = Join-Path $dataDir '.diskunfold.instance.lock'
try {
  $handle = [IO.File]::Open($sentinel, 'Open', 'ReadWrite', 'None')
  $handle.Dispose()
  $result.lockReleased = $true
} catch { $result.lockReleased = $false; $result.lockError = $_.Exception.Message }
$result.filesAfterClose = @(Get-DataFiles)
$result.walLeft = @($result.filesAfterClose | Where-Object { $_ -match '-wal |-shm ' }).Count
$result.stderrBytes = (Get-Item (Join-Path $root 'stderr.txt')).Length
$result.stdoutBytes = (Get-Item (Join-Path $root 'stdout.txt')).Length
# Check a copy so the check cannot create sidecar files next to the real database.
$database = Get-ChildItem -LiteralPath $dataDir -File -Filter '*.db' -ErrorAction SilentlyContinue | Select-Object -First 1
if ($database -and (Get-Command python -ErrorAction SilentlyContinue)) {
  $copy = Join-Path $root 'integrity-copy.db'
  Copy-Item -LiteralPath $database.FullName -Destination $copy
    $result.databaseCheck = (python -c "import sqlite3,sys;c=sqlite3.connect(sys.argv[1]);print('integrity=%s journal_mode_header=%s tables=%d job_states=%s' % (c.execute('pragma integrity_check').fetchone()[0], open(sys.argv[1],'rb').read(20)[18:20].hex(), c.execute('select count(*) from sqlite_master where type=\'table\'').fetchone()[0], [r[0] for r in c.execute('select final_state from job_runtime_job_record')]))" $copy) -join ' '
}
$result.fixture = $root
$result | ConvertTo-Json -Depth 10
$ok = $result.exitCode -eq 0 -and $result.processGone -and $result.noDiskUnfoldProcess -and
  $result.lockReleased -and $result.walLeft -eq 0 -and $result.stderrBytes -eq 0 -and $result.webviewHelpersLeft -eq 0
if (-not $KeepFixture) {
  # Only remove the directory this run created.
  $temp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
  if ([IO.Path]::GetDirectoryName($root) -eq $temp -and [IO.Path]::GetFileName($root) -match '^diskunfold-quit-[0-9a-f-]{36}$' -and $ok) {
    Remove-Item -LiteralPath $root -Recurse -Force
  }
}
if (-not $ok) { throw "Quit smoke failed; evidence retained at $root" }
'Native quit smoke passed.'

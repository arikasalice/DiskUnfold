#Requires -Version 7.4
# Runs the installed Windows UI Automation client against one real WebView2 window.
# Use a GUI build and an interactive Windows session with WebView2 installed.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$Binary)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
if (Get-CimInstance Win32_Process -Filter "Name = 'diskunfold.exe' AND ThreadCount > 0") { throw 'DiskUnfold already running' }
$exe = (Resolve-Path -LiteralPath $Binary).Path
$root = Join-Path ([IO.Path]::GetTempPath()) ('diskunfold-smoke-' + [guid]::NewGuid())
$scanRoot = Join-Path $root 'scope'
$child = Join-Path $scanRoot 'nested'
New-Item -ItemType Directory -Path $child -Force | Out-Null
[IO.File]::WriteAllBytes((Join-Path $scanRoot 'root.bin'), [byte[]]::new(11))
[IO.File]::WriteAllBytes((Join-Path $child 'nested.bin'), [byte[]]::new(7))
$profile = Join-Path $root 'profile'
New-Item -ItemType Directory -Path $profile | Out-Null
$process = Start-Process -FilePath $exe -PassThru -Environment @{ LOCALAPPDATA = $profile } -RedirectStandardError (Join-Path $root 'stderr.txt') -RedirectStandardOutput (Join-Path $root 'stdout.txt')
$scopeTree = [Windows.Automation.TreeScope]::Descendants
$ae = [Windows.Automation.AutomationElement]
$edit = [Windows.Automation.ControlType]::Edit
$button = [Windows.Automation.ControlType]::Button
$textType = [Windows.Automation.ControlType]::Text
function Wait-UI($label, [scriptblock]$probe) {
  $deadline = [DateTime]::UtcNow.AddSeconds(22)
  do {
    if ($process.HasExited) { throw "Application exited while waiting for $label" }
    try { $found = & $probe; if ($found) { return $found } }
    catch [Windows.Automation.ElementNotAvailableException] {}
    Start-Sleep -Milliseconds 80
  } while ([DateTime]::UtcNow -lt $deadline)
  throw "Timed out waiting for $label"
}
function Find-UI($name, $type, $within = $window) {
  $nameCondition = [Windows.Automation.PropertyCondition]::new($ae::NameProperty, $name)
  if ($type) {
    $typeCondition = [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, $type)
    $condition = [Windows.Automation.AndCondition]::new($nameCondition, $typeCondition)
  } else { $condition = $nameCondition }
  $within.FindFirst($scopeTree, $condition)
}
function Wait-Enabled($name, $enabled) {
  Wait-UI "$name enabled=$enabled" { $item = Find-UI $name $button; if ($item -and $item.Current.IsEnabled -eq $enabled) { $item } }
}
function Invoke-UI($item) {
  $pattern = $item.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern)
  $pattern.Invoke()
}
function Set-Scope($path) {
  $item = Find-UI 'Windows folder path' $edit
  $pattern = $item.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern)
  $pattern.SetValue($path)
  $null = Wait-UI "scope input update" {
    $current = (Find-UI 'Windows folder path' $edit).GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern)
    $current.Current.Value -eq $path
  }
}
try {
  $window = Wait-UI 'main window' {
    $process.Refresh()
    if ($process.MainWindowHandle -ne 0) { $ae::FromHandle($process.MainWindowHandle) }
  }
  $null = Wait-UI 'path field' { Find-UI 'Windows folder path' $edit }
  $null = Wait-Enabled 'Start scan' $false
  Set-Scope (Join-Path $root 'missing')
  Invoke-UI (Wait-Enabled 'Review scope' $true)
  $null = Wait-UI 'invalid folder notice' {
    foreach ($msg in @('Check that every selected path is an accessible local folder, then review the scope again.', 'Enter a valid local folder path, then review the scope again.', 'One or more selected paths could not be verified. Review all paths before starting.')) {
      $notice = Find-UI $msg $textType
      if ($notice) { $notice; break }
    }
  }
  $null = Wait-Enabled 'Start scan' $false
  'F01 invalid folder: rejected, Start scan disabled'
  Set-Scope $scanRoot
  Invoke-UI (Wait-Enabled 'Review scope' $true)
  $null = Wait-Enabled 'Start scan' $true
  Set-Scope $child
  $null = Wait-Enabled 'Start scan' $false
  Set-Scope $scanRoot
  $null = Wait-Enabled 'Start scan' $false
  Invoke-UI (Wait-Enabled 'Review scope' $true)
  'F01 changing a reviewed path: new review required'
  Invoke-UI (Wait-Enabled 'Start scan' $true)
  $null = Wait-UI 'Scan finished' { Find-UI 'Scan finished' $textType }
  $null = Wait-UI 'Result version: Final' { Find-UI 'Final' $textType }
  $null = Wait-UI '18-byte logical total' {
    $metric = Find-UI 'Logical size' ([Windows.Automation.ControlType]::Group)
    if ($metric) { Find-UI '18 bytes' $textType $metric }
  }
  'F02/F03 native scan: Scan finished, Result version: Final, 18 bytes'
  Invoke-UI (Wait-Enabled 'Browse folders' $true)
  $null = Wait-UI 'root.bin' { Find-UI 'root.bin' $null }
  $null = Wait-UI '11-byte root row' { Find-UI '11 bytes' $null }
  $nested = Wait-UI 'nested folder button' { Find-UI 'nested' $button }
  Invoke-UI $nested
  $null = Wait-UI 'nested.bin' { Find-UI 'nested.bin' $null }
  $null = Wait-UI '7-byte nested row' { Find-UI '7 bytes' $null }
  Invoke-UI (Wait-Enabled 'Up' $true)
  $null = Wait-UI 'returned to pinned folder' { Find-UI 'root.bin' $null }
  'F05 native browse: root.bin 11 bytes, nested.bin 7 bytes, Up returns to the pinned scope'
} catch {
  Write-Warning "Native smoke failed. Isolated fixture and logs: $root"
  throw
} finally {
  if (-not $process.HasExited) {
    $null = $process.CloseMainWindow()
    if (-not $process.WaitForExit(10000)) { Stop-Process -Id $process.Id; throw 'The test process did not exit' }
  }
}
if ($process.ExitCode -ne 0 -or (Get-Item (Join-Path $root 'stderr.txt')).Length -ne 0) { throw "Abnormal exit, log at $root" }
'Native app exited cleanly.'
$temp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([IO.Path]::DirectorySeparatorChar)
$verifiedRoot = (Resolve-Path -LiteralPath $root).Path
if ([IO.Path]::GetDirectoryName($verifiedRoot) -ne $temp -or
    [IO.Path]::GetFileName($verifiedRoot) -notmatch '^diskunfold-smoke-[0-9a-f-]{36}$') {
  throw "Unexpected fixture directory: $verifiedRoot"
}
# WebView2 subprocesses can retain profile files briefly after the launcher exits.
$cleanupDeadline = [DateTime]::UtcNow.AddSeconds(8)
while (Test-Path -LiteralPath $verifiedRoot) {
  try {
    Remove-Item -LiteralPath $verifiedRoot -Recurse -Force
  } catch [IO.IOException] {
    if ([DateTime]::UtcNow -ge $cleanupDeadline) { throw }
    Start-Sleep -Milliseconds 200
  }
}

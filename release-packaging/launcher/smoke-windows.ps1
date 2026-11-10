#Requires -Version 7.4
# Runs the installed Windows UI Automation client against one real WebView2 window.
# Use a GUI build and an interactive Windows session with WebView2 installed.
[CmdletBinding()]
param(
  [Parameter(Mandatory)][string]$Binary,
  [switch]$VerifyWorkspaceLayout,
  [switch]$VerifyCancellation,
  [switch]$VerifySuspension,
  [switch]$VerifyPopulatedWorkspace,
  [switch]$VerifyNativeFolderDrop
)

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
if ($VerifyCancellation -or $VerifySuspension) {
  $controlScope = Join-Path $root 'control-scope'
  # Give the real filesystem scan enough work to invoke cooperative controls.
  $contents = [byte[]]::new(1)
  for ($group = 0; $group -lt 32; $group++) {
    $folder = Join-Path $controlScope "group-$group"
    [IO.Directory]::CreateDirectory($folder) | Out-Null
    for ($index = 0; $index -lt 800; $index++) {
      [IO.File]::WriteAllBytes((Join-Path $folder "file-$index.bin"), $contents)
    }
  }
}
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
function Wait-VisibleAction($actionName) {
  Wait-UI "$actionName in view" {
    $item = Find-UI $actionName $button
    if ($item -and $item.Current.IsEnabled -and -not $item.Current.IsOffscreen) { $item }
  }
}
# Inspect the UIA viewport before invoking a button, since Invoke can reach clipped controls.
function Wait-ScopeActionsVisible {
  foreach ($name in @('Cancel selection', 'Review scope', 'Start scan')) {
    $null = Wait-UI "$name in view" {
      $item = Find-UI $name $button
      if ($item -and -not $item.Current.IsOffscreen) { $item }
    }
  }
}
function Invoke-UI($item) {
  $pattern = $item.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern)
  $pattern.Invoke()
}
# Open the observed file inspector through its real WebView2 action.
function Test-FileInspection($name, $size) {
  $file = Wait-Enabled $name $true
  $file.SetFocus()
  Invoke-UI (Wait-VisibleAction $name)
  $null = Wait-UI "file attributes for $name" {
    $details = Find-UI 'Observed file attributes' $null
    if ($details) { Find-UI $name $null $details }
  }
  $null = Wait-UI "pinned space for $name" {
    $details = Find-UI 'Observed item space' $null
    if ($details -and (Find-UI 'Observed logical size' $null $details)) {
      $details.FindAll($scopeTree, [Windows.Automation.Condition]::TrueCondition) |
        Where-Object { $_.Current.Name -eq $size -or $_.Current.Name.StartsWith("$size ·") } |
        Select-Object -First 1
    }
  }
  Invoke-UI (Wait-VisibleAction 'Close details')
  $null = Wait-UI "focus returned to $name" {
    $focused = $ae::FocusedElement
    if ($focused -and ($focused.Current.Name -eq $name -or (Find-UI $name $button $focused))) {
      -not (Find-UI 'Close details' $button)
    }
  }
  "F05/F03 native file inspection: $name, $size, focus restored"
}
function Set-Scope($path, $fieldName = 'Windows folder path') {
  $item = Wait-UI $fieldName { Find-UI $fieldName $edit }
  $pattern = $item.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern)
  $pattern.SetValue($path)
  $null = Wait-UI "scope input update" {
    $field = Find-UI $fieldName $edit
    if ($field) {
      $current = $field.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern)
      $current.Current.Value -eq $path
    }
  }
}
# The operator performs the OLE gesture; UIA only observes its result.
function Test-NativeFolderDrop {
  Set-Scope $scanRoot
  Invoke-UI (Wait-Enabled 'Review scope' $true)
  $null = Wait-Enabled 'Start scan' $true
  Write-Host "Within 120 seconds, drag this isolated folder from Explorer onto the Windows folder path field: $child"
  $deadline = [DateTime]::UtcNow.AddSeconds(120)
  $received = $false
  do {
    if ($process.HasExited) { throw 'Application exited while waiting for native folder drop' }
    try {
      $field = Find-UI 'Windows folder path' $edit
      $notice = Find-UI 'Folder selected. Review scope.' $textType
      if ($field -and $notice) {
        $value = $field.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).Current.Value
        if ($value -ceq $child) { $received = $true; break }
      }
    } catch [Windows.Automation.ElementNotAvailableException] {}
    Start-Sleep -Milliseconds 100
  } while ([DateTime]::UtcNow -lt $deadline)
  if (-not $received) { throw 'Native folder drop was not observed with the expected isolated path and drop notice' }
  $null = Wait-Enabled 'Start scan' $false
  Invoke-UI (Wait-Enabled 'Review scope' $true)
  $null = Wait-Enabled 'Start scan' $true
  'F01 native folder drop: isolated folder received, prior review invalidated, fresh review required'
}
# Verify the scope editor and paging in a small native window with real NTFS data.
function Test-PopulatedWorkspace {
  $transform = $window.GetCurrentPattern([Windows.Automation.TransformPattern]::Pattern)
  $transform.Resize(640, 460)
  $null = Wait-UI 'small window' {
    $bounds = $window.Current.BoundingRectangle
    $bounds.Width -le 650 -and $bounds.Height -le 470
  }
  Invoke-UI (Wait-Enabled 'Back to analysis' $true)

  $roots = for ($index = 1; $index -le 6; $index++) {
    $name = "drive-content-$index-" + ('long-segment-' * 6)
    $path = Join-Path $root $name
    [IO.Directory]::CreateDirectory($path) | Out-Null
    $last = if ($index -eq 1) { 239 } else { 0 }
    for ($file = 0; $file -le $last; $file++) {
      [IO.File]::WriteAllBytes((Join-Path $path ('entry-{0:D3}.bin' -f $file)), [byte[]]::new(1))
    }
    $path
  }
  Set-Scope $roots[0]
  for ($index = 1; $index -lt $roots.Count; $index++) {
    Invoke-UI (Wait-Enabled 'Add path' $true)
    Set-Scope $roots[$index] "Additional folder path $($index + 1)"
    Wait-ScopeActionsVisible
  }
  Invoke-UI (Wait-Enabled 'Review scope' $true)
  $null = Wait-Enabled 'Start scan' $true
  Wait-ScopeActionsVisible
  Invoke-UI (Wait-Enabled 'Start scan' $true)
  $null = Wait-UI 'six-root scan finished' { Find-UI 'Scan finished' $textType }
  $available = Wait-UI 'six-root final result or user-controlled switch' {
    if (Find-UI 'Final' $textType) { return 'shown' }
    $switch = Find-UI 'Show latest result' $button
    if ($switch -and $switch.Current.IsEnabled) { return 'switch' }
  }
  if ($available -eq 'switch') {
    $null = Wait-UI 'six-root interim result still pinned' { Find-UI 'Interim' $textType }
    Invoke-UI (Wait-Enabled 'Show latest result' $true)
    'F03 populated workspace: switched from the pinned interim result to the final result'
  }
  try {
    $null = Wait-UI '245-byte aggregate from all six roots' {
      $metric = Find-UI 'Logical size' ([Windows.Automation.ControlType]::Group)
      if ($metric -and (Find-UI 'Final' $textType)) { Find-UI '245 bytes' $textType $metric }
    }
  } catch {
    $metric = Find-UI 'Logical size' ([Windows.Automation.ControlType]::Group)
    if ($metric) {
      $observed = $metric.FindAll($scopeTree, [Windows.Automation.Condition]::TrueCondition) |
        ForEach-Object { $_.Current.Name } | Where-Object { $_ }
      Write-Warning "Six-root logical size UI: $($observed -join ' / ')"
    } else {
      Write-Warning 'Six-root logical size group was absent.'
    }
    throw
  }
  'F01–F03 populated workspace: six long paths, visible scope actions, 245-byte native scan total in a 640x460 window'

  Invoke-UI (Wait-Enabled 'Browse folders' $true)
  $firstRoot = Wait-UI 'large root in the result' {
    $window.FindAll($scopeTree, [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, $button)) |
      Where-Object { $_.Current.Name -like '*drive-content-1-*' } |
      Select-Object -First 1
  }
  Invoke-UI $firstRoot
  $null = Wait-UI 'first observed file' { Find-UI 'entry-000.bin' $null }
  Test-FileInspection 'entry-000.bin' '1 bytes'
  $more = Wait-Enabled 'Load more' $true
  Invoke-UI $more
  $null = Wait-UI 'second page loaded' {
    (Find-UI 'Folder items' $null) -and
      -not (Find-UI 'Load more' $button) -and
      -not (Find-UI 'Loading…' $button) -and
      -not (Find-UI 'Retry page' $button)
  }
  'F05 populated workspace: browsed 240 native files and loaded the second page'
}
# Exercise F02 cancellation while preserving the F03 user's pinned result.
function Test-ScanCancellation($path) {
  Set-Scope $path
  Invoke-UI (Wait-Enabled 'Review scope' $true)
  Invoke-UI (Wait-Enabled 'Start scan' $true)
  Invoke-UI (Wait-Enabled 'Cancel scan' $true)
  $null = Wait-UI 'cancelled scan state' { Find-UI 'Scan status: cancelled' $textType }
  $available = Wait-UI 'cancelled result or user-controlled result switch' {
    $switch = Find-UI 'Show latest result' $button
    if ($switch -and $switch.Current.IsEnabled) { return 'switch' }
    if (Find-UI 'Cancelled' $textType) { return 'shown' }
  }
  if ($available -eq 'switch') {
    $null = Wait-UI 'existing interim result still pinned' { Find-UI 'Interim' $textType }
    'F03 pinned result: cancellation kept the interim result until a manual switch'
    Invoke-UI (Wait-Enabled 'Show latest result' $true)
  }
  $null = Wait-UI 'cancelled result version' { Find-UI 'Cancelled' $textType }
  $null = Wait-UI 'interrupted measurement evidence' {
    $window.FindAll($scopeTree, [Windows.Automation.Condition]::TrueCondition) |
      Where-Object { $_.Current.Name -like 'Measurement interrupted*' } |
      Select-Object -First 1
  }
  'F02 native cancel: cancelled terminal state and interrupted result evidence'
}
# Exercise real pause/resume without treating its acknowledgement as a stopped scan.
function Test-ScanSuspension($path) {
  Set-Scope $path
  Invoke-UI (Wait-Enabled 'Review scope' $true)
  Invoke-UI (Wait-Enabled 'Start scan' $true)
  Invoke-UI (Wait-VisibleAction 'Pause scan')
  $null = Wait-UI 'confirmed paused scan' { Find-UI 'Scan status: paused' $textType }
  $null = Wait-VisibleAction 'Cancel scan'
  Invoke-UI (Wait-VisibleAction 'Resume scan')
  $null = Wait-UI 'resumed scan finished' { Find-UI 'Scan finished' $textType }
  $result = Wait-UI 'final result or manual result switch' {
    if (Find-UI 'Final' $textType) { return 'shown' }
    $switch = Find-UI 'Show latest result' $button
    if ($switch -and $switch.Current.IsEnabled) { return 'switch' }
  }
  if ($result -eq 'switch') { Invoke-UI (Wait-VisibleAction 'Show latest result') }
  $null = Wait-UI 'exact 25 KiB result' {
    $metric = Find-UI 'Logical size' ([Windows.Automation.ControlType]::Group)
    if ($metric -and (Find-UI 'Final' $textType)) { Find-UI '25.0 KiB' $textType $metric }
  }
  'F02 native pause and resume: controls visible, scan paused, final logical size 25 KiB'
}
try {
  $window = Wait-UI 'main window' {
    $process.Refresh()
    if ($process.MainWindowHandle -ne 0) { $ae::FromHandle($process.MainWindowHandle) }
  }
  $null = Wait-UI 'path field' { Find-UI 'Windows folder path' $edit }
  $null = Wait-Enabled 'Start scan' $false
  if ($VerifyNativeFolderDrop) { Test-NativeFolderDrop }
  if ($VerifySuspension) { Test-ScanSuspension $controlScope }
  if ($VerifyCancellation) { Test-ScanCancellation $controlScope }
  Set-Scope (Join-Path $root 'missing')
  Invoke-UI (Wait-Enabled 'Review scope' $true)
  $null = Wait-UI 'invalid folder notice' {
    $notices = @(
      'Check that every selected path is an accessible local folder, then review the scope again.'
      'Enter a valid local folder path, then review the scope again.'
      'One or more selected paths could not be verified. Review all paths before starting.'
      'A selected folder no longer exists. Check the path and review the scope again.'
      'A selected folder path could not be resolved. Check the path and review the scope again.'
      'A selected path needs attention. Check the folder paths and review the scope again.'
    )
    foreach ($msg in $notices) {
      $notice = Find-UI $msg $textType
      if ($notice) { $notice; break }
    }
  }
  $null = Wait-Enabled 'Start scan' $false
  'F01 invalid folder: rejected, Start scan disabled'
  Set-Scope $scanRoot
  if ($VerifyWorkspaceLayout) {
    Invoke-UI (Wait-Enabled 'Add path' $true)
    $null = Wait-UI 'additional path field' { Find-UI 'Additional folder path 2' $edit }
    Wait-ScopeActionsVisible
    Invoke-UI (Wait-Enabled 'Remove path 2' $true)
    $null = Wait-UI 'additional path removed' { -not (Find-UI 'Additional folder path 2' $edit) }
    'F01 workspace: both actions remain in view after adding a path'
  }
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
  Set-Scope $child
  Invoke-UI (Wait-VisibleAction 'Review scope')
  $null = Wait-Enabled 'Start scan' $true
  Invoke-UI (Wait-VisibleAction 'Cancel selection')
  $null = Wait-UI 'discarded scope and retained final total' {
    $field = Find-UI 'Windows folder path' $edit
    $metric = Find-UI 'Logical size' ([Windows.Automation.ControlType]::Group)
    if ($field -and $metric -and (Find-UI 'Final' $textType) -and
        $field.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).Current.Value -eq '') {
      Find-UI '18 bytes' $textType $metric
    }
  }
  $null = Wait-Enabled 'Start scan' $false
  'F01 native selection cancellation: discarded reviewed scope and retained the final 18-byte result'
  Invoke-UI (Wait-Enabled 'Explain volume usage' $true)
  $null = Wait-UI 'volume explanation for the selected root' {
    Find-UI 'Selected scanned folder' $textType
  }
  $null = Wait-UI 'scoped allocated size' { Find-UI 'Scoped allocated size' $null }
  $null = Wait-UI 'scoped unique physical size' { Find-UI 'Scoped unique physical size' $null }
  Invoke-UI (Wait-Enabled 'Hide volume explanation' $true)
  $null = Wait-UI 'closed volume explanation' { -not (Find-UI 'Selected scanned folder' $textType) }
  'F03 native volume explanation: scoped allocation and unique physical evidence, then closed'
  Invoke-UI (Wait-Enabled 'View folder details' $true)
  $null = Wait-UI 'pinned folder space detail' {
    $details = Find-UI 'Observed item space' $null
    if ($details) { Find-UI 'Observed logical size' $null $details }
  }
  Invoke-UI (Wait-Enabled 'Hide folder details' $true)
  $null = Wait-UI 'closed pinned folder details' { -not (Find-UI 'Observed item space' $null) }
  'F03 native item space: opened and closed the pinned folder observations'
  Invoke-UI (Wait-Enabled 'Browse folders' $true)
  $null = Wait-UI 'root.bin' { Find-UI 'root.bin' $null }
  $null = Wait-UI '11-byte root row' { Find-UI '11 bytes' $null }
  Test-FileInspection 'root.bin' '11 bytes'
  $nested = Wait-UI 'nested folder button' { Find-UI 'nested' $button }
  Invoke-UI $nested
  $null = Wait-UI 'nested.bin' { Find-UI 'nested.bin' $null }
  $null = Wait-UI '7-byte nested row' { Find-UI '7 bytes' $null }
  Invoke-UI (Wait-Enabled 'Up' $true)
  $null = Wait-UI 'returned to pinned folder' { Find-UI 'root.bin' $null }
  'F05 native browse: root.bin 11 bytes, nested.bin 7 bytes, Up returns to the pinned scope'
  if ($VerifyPopulatedWorkspace) { Test-PopulatedWorkspace }
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

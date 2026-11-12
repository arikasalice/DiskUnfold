#Requires -Version 7.4
# Runs the installed Windows UI Automation client against one real WebView2 window.
# Use a GUI build and an interactive Windows session with WebView2 installed.
[CmdletBinding()]
param(
  [Parameter(Mandatory)][string]$Binary,
  [switch]$VerifyWorkspaceLayout,
  [switch]$VerifyInitialDrive,
  [switch]$VerifyAutomaticScopeReview,
  [switch]$VerifyCancellation,
  [switch]$VerifySuspension,
  [switch]$VerifyPopulatedWorkspace,
  [switch]$VerifyPhysicalAccounting,
  [switch]$VerifyDeniedSubtree,
  [switch]$VerifyJunctionBoundary,
  [switch]$VerifyReplacedScopeRoot,
  [switch]$VerifySettingsRestoration,
  [switch]$VerifyOperationHistory,
  [switch]$VerifyInspectionPathDisclosure,
  [switch]$VerifyInspectionKeyboard,
  [switch]$VerifyPathCopyContention,
  [switch]$VerifyRecentScope,
  [switch]$VerifyNativeFolderDrop
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;

namespace DiskUnfoldSmoke {
  public static class Keyboard {
    [StructLayout(LayoutKind.Sequential)]
    private struct KeyboardInput {
      public ushort VirtualKey, ScanCode;
      public uint Flags, Time;
      public UIntPtr ExtraInfo;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct MouseInput {
      public int X, Y;
      public uint Data, Flags, Time;
      public UIntPtr ExtraInfo;
    }

    [StructLayout(LayoutKind.Explicit)]
    private struct InputData {
      [FieldOffset(0)] public KeyboardInput Keyboard;
      [FieldOffset(0)] public MouseInput Mouse;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct Input {
      public uint Type;
      public InputData Data;
    }

    [DllImport("user32.dll")]
    private static extern IntPtr GetForegroundWindow();

    [DllImport("user32.dll", SetLastError = true)]
    private static extern uint SendInput(uint count, Input[] inputs, int size);

    // Use the Windows input queue so WebView2 handles a down/up sequence.
    public static void Press(IntPtr window, ushort key) {
      if (window == IntPtr.Zero || GetForegroundWindow() != window)
        throw new InvalidOperationException("DiskUnfold must own the foreground window.");
      const uint KeyboardType = 1;
      const uint KeyUp = 2;
      var inputs = new Input[2];
      inputs[0].Type = inputs[1].Type = KeyboardType;
      inputs[0].Data.Keyboard.VirtualKey = inputs[1].Data.Keyboard.VirtualKey = key;
      inputs[1].Data.Keyboard.Flags = KeyUp;
      if (SendInput(2, inputs, Marshal.SizeOf<Input>()) != 2)
        throw new Win32Exception(Marshal.GetLastWin32Error(), "Could not send both keyboard events.");
    }
  }
}
'@
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
function Wait-UI($label, [scriptblock]$probe, $seconds = 22) {
  $deadline = [DateTime]::UtcNow.AddSeconds($seconds)
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
  $pattern = $null
  if (-not $item.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
    throw "Action '$($item.Current.Name)' does not expose InvokePattern"
  }
  $pattern.Invoke()
}
# WebView2 may expose an aria-expanded button as a disclosure instead of Invoke.
function Expand-UI($item) {
  $disclosure = $null
  if ($item.TryGetCurrentPattern([Windows.Automation.ExpandCollapsePattern]::Pattern, [ref]$disclosure)) {
    $disclosure.Expand()
  } else {
    Invoke-UI $item
  }
}
function Wait-Details($name, $contentName) {
  $ready = Wait-UI "$contentName or its disclosure" {
    if (Find-UI $contentName $null) { return 'content' }
    $item = Find-UI $name $button
    if ($item -and $item.Current.IsEnabled) { return $item }
  }
  if ($ready -is [Windows.Automation.AutomationElement]) { Expand-UI $ready }
}
# Read all observed bases through the result's comparison disclosure.
function Find-MeasurementDetails($basis) {
  $toggle = Find-UI 'Measurement details' $button
  if (-not $toggle) { return }
  $pattern = $null
  if ($toggle.TryGetCurrentPattern([Windows.Automation.ExpandCollapsePattern]::Pattern, [ref]$pattern) -and
      $pattern.Current.ExpandCollapseState -eq [Windows.Automation.ExpandCollapseState]::Collapsed) {
    $pattern.Expand()
  }
  $window.FindAll($scopeTree, [Windows.Automation.Condition]::TrueCondition) |
    Where-Object { $_.Current.Name -eq 'Measurement details' -and $_.Current.ControlType -ne $button -and (Find-UI $basis $textType $_) } |
    Select-Object -First 1
}
function Get-MeasurementNames($details, $basis) {
  $names = @($details.FindAll($scopeTree, [Windows.Automation.Condition]::TrueCondition) |
    ForEach-Object { $_.Current.Name })
  $labels = @('Logical size', 'Allocated size', 'Unique physical size')
  $start = [Array]::IndexOf($names, $basis)
  for ($index = $start + 1; $index -lt $names.Count; $index++) {
    $value = $names[$index]
    if ($value -ne $basis -and $value -in $labels) { break }
    $value
  }
}
function Find-LogicalSize($expected) {
  $metric = Find-UI 'Logical size' ([Windows.Automation.ControlType]::Group)
  if ($metric) { return Find-UI $expected $textType $metric }
  $details = Find-MeasurementDetails 'Logical size'
  if ($details -and ((Get-MeasurementNames $details 'Logical size') -contains $expected)) {
    return $details
  }
}
# Wait for the expected total, selecting a newer result if the interim one stays pinned.
function Wait-FinalLogicalSize($expected, $versionLabel = 'Final') {
  $selection = @{ LastSwitch = [DateTime]::MinValue }
  Wait-UI "final logical size $expected" {
    if (-not (Find-UI 'Scan finished' $textType)) { return }
    if (Find-UI $versionLabel $textType) {
      $value = Find-LogicalSize $expected
      if ($value) { return $value }
    }
    $latest = Find-UI 'Show latest result' $button
    $now = [DateTime]::UtcNow
    if ($latest -and $latest.Current.IsEnabled -and $now -ge $selection.LastSwitch.AddSeconds(1)) {
      Invoke-UI $latest
      $selection.LastSwitch = $now
    }
  }
}
# Send a key only while the test window owns the foreground.
function Send-SmokeKey($key) {
  $process.Refresh()
  [DiskUnfoldSmoke.Keyboard]::Press($process.MainWindowHandle, $key)
}
# The grid uses a row as its keyboard stop. File buttons stay out of the tab order.
function Find-InspectionRow($file) {
  $element = [Windows.Automation.TreeWalker]::RawViewWalker.GetParent($file)
  while ($element -and $element.Current.ControlType -ne [Windows.Automation.ControlType]::DataItem) {
    $element = [Windows.Automation.TreeWalker]::RawViewWalker.GetParent($element)
  }
  if (-not $element -or -not $element.Current.IsKeyboardFocusable) {
    throw 'The file row is not keyboard focusable'
  }
  return $element
}
# A virtual row can own focus without its file button being a tab stop.
function Get-FocusedInspectionRow($name) {
  $focused = $ae::FocusedElement
  if ($focused -and $focused.Current.ControlType -eq [Windows.Automation.ControlType]::DataItem -and
      (Find-UI $name $button $focused)) {
    $focused
  }
}
# Open the observed file inspector through its real WebView2 action.
function Test-FileInspection($name, $size, $testCopyContention = $false, $expectedPath = '') {
  $file = Wait-Enabled $name $true
  if ($VerifyInspectionKeyboard) {
    $opener = Find-InspectionRow $file
    $openerId = $opener.GetRuntimeId() -join ','
    $opener.SetFocus()
    $null = Wait-UI "keyboard focus on $name" {
      $focused = Get-FocusedInspectionRow $name
      $focused -and ($focused.GetRuntimeId() -join ',') -eq $openerId -and -not $focused.Current.IsOffscreen
    }
    Send-SmokeKey 0x0D # VK_RETURN
  } else {
    $file.SetFocus()
    Invoke-UI (Wait-VisibleAction $name)
  }
  $null = Wait-UI "file attributes for $name" {
    $details = Find-UI 'Observed file attributes' $null
    if ($details) { Find-UI $name $null $details }
  }
  if ($testCopyContention) { Test-PathCopyContention }
  if ($expectedPath) { Test-InspectionPathDisclosure $expectedPath }
  Wait-Details 'Space measurement details' 'Observed item space'
  $null = Wait-UI "pinned space for $name" {
    $details = Find-UI 'Observed item space' $null
    if ($details -and (Find-UI 'Observed logical size' $null $details)) {
      $details.FindAll($scopeTree, [Windows.Automation.Condition]::TrueCondition) |
        Where-Object { $_.Current.Name -eq $size -or $_.Current.Name.StartsWith("$size ·") } |
        Select-Object -First 1
    }
  }
  if ($VerifyInspectionKeyboard) {
    Send-SmokeKey 0x1B # VK_ESCAPE
    $null = Wait-UI "file details dismissed by Escape for $name" {
      -not (Find-UI 'Close details' $button)
    }
  } else {
    Invoke-UI (Wait-VisibleAction 'Close details')
  }
  try {
    $null = Wait-UI "focus returned to $name" {
      $focused = $ae::FocusedElement
      if ($focused -and ($focused.Current.Name -eq $name -or (Find-UI $name $button $focused))) {
        -not (Find-UI 'Close details' $button)
      }
    }
  } catch {
    $focused = $ae::FocusedElement
    if ($focused) { Write-Warning "Focused element after close: $($focused.Current.ControlType.ProgrammaticName), $($focused.Current.Name)" }
    throw
  }
  $activation = if ($VerifyInspectionKeyboard) { 'Enter/Escape' } else { 'UI Automation' }
  "F05/F03 native file inspection ($activation): $name, $size, focus restored"
}

# Verify the restored row and viewport position using Windows UI Automation.
function Test-PagedInspectionKeyboard($name, $path) {
  (Find-InspectionRow (Wait-Enabled 'entry-000.bin' $true)).SetFocus()
  $null = Wait-UI 'first row focused before End' { Get-FocusedInspectionRow 'entry-000.bin' }
  Send-SmokeKey 0x23 # VK_END
  $null = Wait-UI 'last row focused on the second page' { Get-FocusedInspectionRow 'entry-239.bin' }
  Send-SmokeKey 0x26 # VK_UP
  $before = Wait-UI 'long file name focused' { Get-FocusedInspectionRow $name }
  $runtimeId = $before.GetRuntimeId() -join ','
  $rowTop = $before.Current.BoundingRectangle.Top
  if ($before.Current.IsOffscreen) { throw 'The long file name row is offscreen before inspection' }

  Test-FileInspection $name '1 bytes' $false $path
  $after = Wait-UI 'same second-page row focused after Escape' { Get-FocusedInspectionRow $name }
  if (($after.GetRuntimeId() -join ',') -ne $runtimeId -or $after.Current.IsOffscreen -or
      [Math]::Abs($after.Current.BoundingRectangle.Top - $rowTop) -gt 2) {
    throw 'Inspection did not restore the same native row and viewport position'
  }
  Send-SmokeKey 0x28 # VK_DOWN
  $null = Wait-UI 'next file focused after inspection' { Get-FocusedInspectionRow 'entry-239.bin' }
  'F05 paged keyboard inspection: long file name, same row and viewport position, Down reaches next file'
}
# The compact inspector should make the whole path available on demand.
function Test-InspectionPathDisclosure($expectedPath) {
  $attributes = Find-UI 'Observed file attributes' $null
  $pathSuffix = $expectedPath
  if ($pathSuffix.StartsWith('\\?\')) { $pathSuffix = $pathSuffix.Substring(4) }
  $findPath = {
    $attributes.FindAll($scopeTree, [Windows.Automation.Condition]::TrueCondition) |
      Where-Object { ([string]$_.Current.Name).EndsWith($pathSuffix, [StringComparison]::OrdinalIgnoreCase) } |
      Select-Object -First 1
  }
  if (& $findPath) { throw 'The long path was already expanded' }
  Expand-UI (Wait-Enabled 'Full path' $true)
  $path = Wait-UI 'full observed path after disclosure' $findPath
  # WebView2 may expose the path as a text leaf inside its focusable paragraph.
  if (-not $path.Current.IsKeyboardFocusable) {
    $path = [Windows.Automation.TreeWalker]::RawViewWalker.GetParent($path)
  }
  if (-not $path -or -not $path.Current.IsKeyboardFocusable) { throw 'The full path is not keyboard focusable' }
  $toggle = Wait-Enabled 'Full path' $true
  $disclosure = $null
  if ($toggle.TryGetCurrentPattern([Windows.Automation.ExpandCollapsePattern]::Pattern, [ref]$disclosure)) {
    $disclosure.Collapse()
  } else {
    Invoke-UI $toggle
  }
  $null = Wait-UI 'collapsed full path' { -not (& $findPath) }
  'F05 native file inspection: full path expanded, focusable, and collapsed'
}
# Hold the clipboard open in the test process without reading or clearing it.
function Test-PathCopyContention {
  if (-not ('DiskUnfoldSmoke.ClipboardLock' -as [type])) {
    Add-Type -Namespace DiskUnfoldSmoke -Name ClipboardLock -MemberDefinition @'
[System.Runtime.InteropServices.DllImport("user32.dll", SetLastError = true)]
public static extern bool OpenClipboard(System.IntPtr window);
[System.Runtime.InteropServices.DllImport("user32.dll", SetLastError = true)]
public static extern bool CloseClipboard();
'@
  }
  $copy = Wait-VisibleAction 'Copy path'
  if (-not [DiskUnfoldSmoke.ClipboardLock]::OpenClipboard([IntPtr]::Zero)) {
    throw 'Could not acquire the clipboard for the contention test'
  }
  try {
    Invoke-UI $copy
    $null = Wait-UI 'clipboard write refusal' { Find-UI 'Could not copy to the clipboard.' $null } 4
  } finally {
    if (-not [DiskUnfoldSmoke.ClipboardLock]::CloseClipboard()) {
      throw 'Could not release the clipboard contention test lock'
    }
  }
  $null = Wait-Enabled 'Copy path' $true
  $null = Wait-Enabled 'Close details' $true
  'F05 native path copy: clipboard contention reported and file detail stayed open'
}
# Exercise the row menu without changing or inspecting clipboard contents.
function Test-BrowseCopyContention($name) {
  Expand-UI (Wait-VisibleAction "Actions for $name")
  $copy = Wait-UI 'row copy menu item' {
    Find-UI 'Copy path' ([Windows.Automation.ControlType]::MenuItem)
  }
  if (-not [DiskUnfoldSmoke.ClipboardLock]::OpenClipboard([IntPtr]::Zero)) {
    throw 'Could not acquire the clipboard for the row menu test'
  }
  try {
    Invoke-UI $copy
    $notice = Wait-UI 'visible row copy refusal' {
      # The structured filename span and the reason are separate UIA text nodes.
      $message = Find-UI ': Could not copy to the clipboard.' $null
      if ($message -and -not $message.Current.IsOffscreen) { $message }
    } 4
    if ($notice.Current.BoundingRectangle.IsEmpty) { throw 'Row copy feedback has no visible bounds' }
  } finally {
    if (-not [DiskUnfoldSmoke.ClipboardLock]::CloseClipboard()) {
      throw 'Could not release the row menu clipboard lock'
    }
  }
  $null = Wait-UI 'closed row menu' { -not (Find-UI 'Copy path' ([Windows.Automation.ControlType]::MenuItem)) }
  $null = Wait-VisibleAction $name
  "F05 native row copy: $name menu reported clipboard contention in visible feedback"
}
# Use the observed folder reference without navigating into the folder.
function Test-BrowseFolderInspection($name) {
  Expand-UI (Wait-VisibleAction "Actions for $name")
  $inspect = Wait-UI 'folder inspect menu item' {
    Find-UI 'Inspect' ([Windows.Automation.ControlType]::MenuItem)
  }
  Invoke-UI $inspect
  $null = Wait-UI "observed attributes of $name" {
    $attributes = Find-UI 'Observed folder attributes' $null
    if ($attributes) { Find-UI $name $null $attributes }
  }
  Invoke-UI (Wait-VisibleAction 'Close details')
  $null = Wait-UI 'returned to the same folder row' {
    $focused = $ae::FocusedElement
    if ($focused -and ($focused.Current.Name -eq $name -or (Find-UI $name $button $focused))) {
      -not (Find-UI 'Close details' $button)
    }
  }
  "F05 native folder inspection: $name attributes shown and row focus restored"
}
# Scan an isolated file and check both unit systems across the pinned workspace.
function Test-ScaledFileInspection {
  $scaledScope = Join-Path $root 'scaled-scope'
  [IO.Directory]::CreateDirectory($scaledScope) | Out-Null
  [IO.File]::WriteAllBytes((Join-Path $scaledScope 'scaled.bin'), [byte[]]::new(1100))
  Invoke-UI (Wait-Enabled 'Back to analysis' $true)
  Open-TargetEditor
  Invoke-UI (Wait-Enabled 'Cancel selection' $true)
  Set-Scope $scaledScope
  Invoke-UI (Wait-Enabled 'Review scope' $true)
  Invoke-UI (Wait-Enabled 'Start scan' $true)
  $null = Wait-FinalLogicalSize '1.1 KiB'
  Invoke-UI (Wait-Enabled 'Browse folders' $true)
  Test-FileInspection 'scaled.bin' '1.1 KiB · 1,100 bytes'
  Invoke-UI (Wait-Enabled 'Back to analysis' $true)
  Wait-Details 'Display options' 'Unit system'
  $unitSystem = Wait-UI 'unit system selector' {
    Find-UI 'Unit system' ([Windows.Automation.ControlType]::ComboBox)
  }
  $unitSystem.SetFocus()
  $null = Wait-UI 'keyboard focus on the unit system selector' {
    $focused = Find-UI 'Unit system' ([Windows.Automation.ControlType]::ComboBox)
    $focused -and $focused.Current.HasKeyboardFocus -and -not $focused.Current.IsOffscreen
  }
  # The closed native selector moves directly to its last option, SI.
  Send-SmokeKey 0x23 # VK_END
  $null = Wait-FinalLogicalSize '1.1 kB'
  Invoke-UI (Wait-Enabled 'Browse folders' $true)
  $null = Wait-UI 'pinned folder row in SI units' {
    $grid = Find-UI 'Folder items' $null
    if ($grid) { Find-UI '1.1 kB' $null $grid }
  }
  Test-FileInspection 'scaled.bin' '1.1 kB · 1,100 bytes'
  Invoke-UI (Wait-Enabled 'Back to analysis' $true)
  $null = Wait-FinalLogicalSize '1.1 kB'
  Invoke-UI (Wait-Enabled 'Browse folders' $true)
  'F03/F05 native display units: SI choice kept across analysis, folder rows and file inspection'
}
# Read the original count only from a complete result observation, regardless of IEC rounding.
function Read-CompleteSize($basis) {
  $details = Wait-UI "$basis result measurement" { Find-MeasurementDetails $basis }
  $names = @(Get-MeasurementNames $details $basis)
  if (-not ($names -cmatch '(?:^| · )Complete measurement(?: · |$)')) {
    throw "$basis was not complete: $($names -join ' / ')"
  }
  foreach ($part in ($names -split '·')) {
    if ($part.Trim() -match '^(?<bytes>[0-9][0-9,\.\p{Zs}]*) bytes$') {
      return [uint64]($Matches.bytes -replace '[^0-9]', '')
    }
  }
  throw "No original byte count for $basis`: $($names -join ' / ')"
}

# Compare the same NTFS allocation before and after adding a second name in a sibling folder.
function Test-PhysicalAccounting {
  $accounting = Join-Path $root 'physical-accounting'
  $left = Join-Path $accounting 'left'
  $right = Join-Path $accounting 'right'
  $null = [IO.Directory]::CreateDirectory($left)
  $null = [IO.Directory]::CreateDirectory($right)
  $original = Join-Path $left 'original.bin'
  $contents = [byte[]]::new(1MB)
  [Random]::new(2026).NextBytes($contents)
  [IO.File]::WriteAllBytes($original, $contents)

  Invoke-UI (Wait-Enabled 'Back to analysis' $true)
  foreach ($expected in @('1.0 MiB', '2.0 MiB')) {
    Open-TargetEditor
    Invoke-UI (Wait-Enabled 'Cancel selection' $true)
    Set-Scope $accounting
    Invoke-UI (Wait-Enabled 'Review scope' $true)
    Invoke-UI (Wait-Enabled 'Start scan' $true)
    $null = Wait-FinalLogicalSize $expected
    if ($expected -eq '1.0 MiB') {
      $singleAllocated = Read-CompleteSize 'Allocated size'
      if ($singleAllocated -eq 0 -or (Read-CompleteSize 'Logical size') -ne 1MB) {
        throw 'The original file did not provide the expected allocation evidence'
      }
      $null = New-Item -ItemType HardLink -Path (Join-Path $right 'second-name.bin') -Target $original
    } else {
      $logical = Read-CompleteSize 'Logical size'
      $allocated = Read-CompleteSize 'Allocated size'
      if ($logical -ne 2MB -or $allocated -ne $singleAllocated) {
        throw "Hardlink totals differed: logical $logical, allocated $allocated, single allocation $singleAllocated"
      }
      Invoke-UI (Wait-Enabled 'Explain volume usage' $true)
      Wait-Details 'Observation and difference details' 'Deduplicated allocation'
      $raw = $allocated.ToString('N0', [Globalization.CultureInfo]::InvariantCulture)
      $null = Wait-UI 'measured hardlink deduplication in volume difference' {
        $details = Find-UI 'Volume difference' $null
        if ($details) {
          $details.FindAll($scopeTree, [Windows.Automation.Condition]::TrueCondition) |
            Where-Object { $_.Current.Name -like "*$raw bytes · Measured" } |
            Select-Object -First 1
        }
      }
      Invoke-UI (Wait-Enabled 'Hide volume explanation' $true)
      "F03 native NTFS hardlinks: logical $logical bytes across two paths; allocated $allocated bytes once"
      "F03 native volume difference: deduplicated allocation $allocated bytes (Measured)"
    }
  }
}
# Reopen target selection when the analysis workspace shows a pinned result.
function Open-TargetEditor {
  $entry = Wait-UI 'target input or reanalysis action' {
    $field = Find-UI 'Windows folder path' $edit
    if ($field) { return $field }
    $opener = Find-UI 'Analyze again' $button
    if ($opener -and $opener.Current.IsEnabled) { return $opener }
  }
  if ($entry.Current.ControlType -eq $button) {
    Expand-UI $entry
    $null = Wait-UI 'reopened target input' { Find-UI 'Windows folder path' $edit }
  }
}
function Set-Scope($path, $fieldName = 'Windows folder path') {
  Open-TargetEditor
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
# Exercise scope admission from a folder edit without invoking the review action.
function Test-AutomaticScopeReview {
  Set-Scope $child
  $null = Wait-UI 'automatically reviewed scope' { Find-UI 'Reviewed scope' $null }
  Invoke-UI (Wait-Enabled 'Start scan' $true)
  $null = Wait-FinalLogicalSize '7 bytes'
  'F01 native automatic review: folder edit enabled scan without invoking Review scope; pinned logical size 7 bytes'
}
# Exercise recent-scope reopen in a build with the storage policy supplied.
function Test-RecentScopeReopen {
  Open-TargetEditor
  $original = $window.Current.BoundingRectangle
  $transform = $window.GetCurrentPattern([Windows.Automation.TransformPattern]::Pattern)
  $transform.Resize(640, 460)
  try {
    Expand-UI (Wait-Enabled 'Recent analysis ranges' $true)
    Invoke-UI (Wait-Enabled 'Review range 1' $true)
    $null = Wait-UI 'stored volume matched' { Find-UI 'Volume matched' $textType }
    Invoke-UI (Wait-Enabled 'Use this range' $true)
    $null = Wait-UI 'visible focused replacement in small window' {
      $focused = $ae::FocusedElement
      if ($focused.Current.Name -eq 'Replace current scope' -and -not $focused.Current.IsOffscreen) { $focused }
    }
    Invoke-UI (Wait-Enabled 'Keep editing' $true)
    $use = Wait-UI 'visible focused apply action after keeping edits' {
      $focused = $ae::FocusedElement
      if ($focused.Current.Name -eq 'Use this range' -and -not $focused.Current.IsOffscreen) { $focused }
    }
    Invoke-UI $use
    $confirm = Wait-Enabled 'Replace current scope' $true
    Invoke-UI $confirm
    'F01 native recent scope: confirmation and return actions keep visible focus in the small window'
  } finally {
    $transform.Resize($original.Width, $original.Height)
  }
  $null = Wait-Enabled 'Start scan' $true
  if (-not (Find-LogicalSize '18 bytes')) { throw 'Applying the saved range replaced the pinned result' }
  [IO.File]::WriteAllBytes((Join-Path $scanRoot 'root.bin'), [byte[]]::new(16))
  Invoke-UI (Wait-Enabled 'Start scan' $true)
  $null = Wait-FinalLogicalSize '23 bytes'
  'F01 native recent scope: matched saved volume, preserved pinned 18-byte result until explicit scan, then read 23 bytes'

  [IO.File]::WriteAllBytes((Join-Path $scanRoot 'root.bin'), [byte[]]::new(11))
  Set-Scope $child
  Set-Scope $scanRoot
  Invoke-UI (Wait-Enabled 'Review scope' $true)
  Invoke-UI (Wait-Enabled 'Start scan' $true)
  $null = Wait-FinalLogicalSize '18 bytes'
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

  $longFileName = 'entry-238-' + ('long-name-' * 10) + 'end.bin'
  $roots = for ($index = 1; $index -le 6; $index++) {
    $name = "drive-content-$index-" + ('long-segment-' * 6)
    $path = Join-Path $root $name
    [IO.Directory]::CreateDirectory($path) | Out-Null
    $last = if ($index -eq 1) { 239 } else { 0 }
    for ($file = 0; $file -le $last; $file++) {
      $fileName = if ($VerifyInspectionKeyboard -and $file -eq 238) { $longFileName } else { 'entry-{0:D3}.bin' -f $file }
      [IO.File]::WriteAllBytes((Join-Path $path $fileName), [byte[]]::new(1))
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
  try {
    $null = Wait-FinalLogicalSize '245 bytes'
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
  Expand-UI (Wait-Enabled 'Observation details' $true)
  $null = Wait-UI 'pinned observation and full folder location' {
    $details = Find-UI 'Full folder location and observation' $null
    $details -and (Find-UI 'Location hierarchy' $null $details) -and
      (Find-UI 'Pinned result' $null $details)
  }
  $toggle = Wait-Enabled 'Observation details' $true
  $disclosure = $null
  if ($toggle.TryGetCurrentPattern([Windows.Automation.ExpandCollapsePattern]::Pattern, [ref]$disclosure)) {
    $disclosure.Collapse()
  } else {
    Invoke-UI $toggle
  }
  $null = Wait-UI 'collapsed folder observations with the first row retained' {
    -not (Find-UI 'Full folder location and observation' $null) -and
      (Find-UI 'entry-000.bin' $null)
  }
  $null = Wait-VisibleAction 'Back to analysis'
  'F05 compact workspace: observation details open and close, the first row remains, and Back to analysis stays visible'
  if ($VerifyInspectionPathDisclosure) {
    Test-FileInspection 'entry-000.bin' '1 bytes' $false (Join-Path $roots[0] 'entry-000.bin')
  } else {
    Test-FileInspection 'entry-000.bin' '1 bytes'
  }
  $more = Wait-Enabled 'Load more' $true
  Invoke-UI $more
  $null = Wait-UI 'second page loaded' {
    (Find-UI 'Folder items' $null) -and
      -not (Find-UI 'Load more' $button) -and
      -not (Find-UI 'Loading…' $button) -and
      -not (Find-UI 'Retry page' $button)
  }
  'F05 populated workspace: browsed 240 native files and loaded the second page'
  if ($VerifyInspectionKeyboard) {
    Test-PagedInspectionKeyboard $longFileName (Join-Path $roots[0] $longFileName)
  }
}
# Exercise F02 cancellation while preserving the F03 user's pinned result.
function Test-ScanCancellation($path) {
  Set-Scope $path
  Invoke-UI (Wait-Enabled 'Review scope' $true)
  Invoke-UI (Wait-Enabled 'Start scan' $true)
  Invoke-UI (Wait-Enabled 'Cancel scan' $true)
  $null = Wait-UI 'cancelled scan state' { Find-UI 'Scan cancelled' $textType }
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
  $null = Wait-UI 'confirmed paused scan' { Find-UI 'Scan paused' $textType }
  $null = Wait-VisibleAction 'Cancel scan'
  Invoke-UI (Wait-VisibleAction 'Resume scan')
  $null = Wait-FinalLogicalSize '25.0 KiB'
  'F02 native pause and resume: controls visible, scan paused, final logical size 25 KiB'
}
# Select and review a real drive without starting a scan of the entire volume.
function Test-InitialDrive {
  $drive = [IO.Path]::GetPathRoot($root)
  $original = $window.Current.BoundingRectangle
  $transform = $window.GetCurrentPattern([Windows.Automation.TransformPattern]::Pattern)
  $transform.Resize(640, 460)
  try {
    $option = Wait-UI 'visible fixture drive in the small window' {
      $bounds = $window.Current.BoundingRectangle
      if ($bounds.Width -gt 650 -or $bounds.Height -gt 470) { return }
      $radio = Find-UI $drive ([Windows.Automation.ControlType]::RadioButton)
      if ($radio -and $radio.Current.IsEnabled -and -not $radio.Current.IsOffscreen) { $radio }
    }
    $null = Wait-UI 'initial path visible' {
      $field = Find-UI 'Windows folder path' $edit
      if ($field -and -not $field.Current.IsOffscreen) { $field }
    }
    Wait-ScopeActionsVisible
    $option.GetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern).Select()
    $null = Wait-UI 'drive path selected' {
      $field = Find-UI 'Windows folder path' $edit
      $field -and $field.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).Current.Value -eq $drive
    }
    $null = Wait-Enabled 'Start scan' $false
    Invoke-UI (Wait-VisibleAction 'Review scope')
    $null = Wait-VisibleAction 'Start scan'
    Invoke-UI (Wait-VisibleAction 'Cancel selection')
    $null = Wait-Enabled 'Start scan' $false
    'F01 initial drive: native drive selected and reviewed in the small window; scan not started'
  } finally { $transform.Resize($original.Width, $original.Height) }
}
# Confirm one denied subtree does not hide accessible bytes or become an exact total.
function Test-DeniedSubtree {
  $scope = Join-Path $root 'permission-scope'
  $denied = Join-Path $scope 'denied-child'
  New-Item -ItemType Directory -Path $denied -Force | Out-Null
  [IO.File]::WriteAllBytes((Join-Path $scope 'observed.bin'), [byte[]]::new(13))
  [IO.File]::WriteAllBytes((Join-Path $denied 'unobserved.bin'), [byte[]]::new(37))
  $original = Get-Acl -LiteralPath $denied
  $acl = Get-Acl -LiteralPath $denied
  $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new(
    [Security.Principal.WindowsIdentity]::GetCurrent().User, 'ListDirectory', 'Deny'))
  try {
    Set-Acl -LiteralPath $denied -AclObject $acl
    Set-Scope $scope
    Invoke-UI (Wait-Enabled 'Review scope' $true)
    Invoke-UI (Wait-Enabled 'Start scan' $true)
    $null = Wait-FinalLogicalSize 'At least 13 bytes' 'Final with scan gaps'
    Invoke-UI (Wait-Enabled 'View scan issues' $true)
    $null = Wait-UI 'access denied evidence' {
      $issues = Find-UI 'Scan issue entries' $null
      if ($issues) {
        $names = $issues.FindAll($scopeTree, [Windows.Automation.Condition]::TrueCondition) |
          ForEach-Object { $_.Current.Name }
        ($names -contains 'Unread ranges') -and ($names -contains 'Opening a folder') -and
          ($names -contains 'Access denied') -and ($names -like '*\denied-child')
      }
    }
    Invoke-UI (Wait-Enabled 'Hide scan issues' $true)
    'F02/F03 denied NTFS subtree: access denied, pinned logical lower bound 13 bytes'
  } finally { Set-Acl -LiteralPath $denied -AclObject $original }
}
# A junction is a recorded link, so its target cannot contribute to the pinned total.
function Test-JunctionBoundary {
  $fixture = Join-Path $root 'junction-fixture'
  $scope = Join-Path $fixture 'scope'
  $outside = Join-Path $fixture 'outside'
  $junction = Join-Path $scope 'redirect'
  [IO.Directory]::CreateDirectory($scope) | Out-Null
  [IO.Directory]::CreateDirectory($outside) | Out-Null
  [IO.File]::WriteAllBytes((Join-Path $scope 'observed.bin'), [byte[]]::new(19))
  [IO.File]::WriteAllBytes((Join-Path $outside 'outside-marker.bin'), [byte[]]::new(1024))
  Push-Location -LiteralPath $fixture
  try {
    & cmd.exe /D /C mklink /J 'scope\redirect' outside | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Could not create the isolated NTFS junction' }
  } finally { Pop-Location }
  try {
    Set-Scope $scope
    Invoke-UI (Wait-Enabled 'Review scope' $true)
    Invoke-UI (Wait-Enabled 'Start scan' $true)
    # The scan is final even though the link limits measurement completeness.
    $null = Wait-FinalLogicalSize 'At least 19 bytes'
    Invoke-UI (Wait-Enabled 'View scan issues' $true)
    $issues = Wait-UI 'junction issue list' { Find-UI 'Scan issue entries' $null }
    $names = @($issues.FindAll($scopeTree, [Windows.Automation.Condition]::TrueCondition) |
      ForEach-Object { $_.Current.Name } | Where-Object { $_ })
    $issueText = $names -join ' '
    $issueRows = @(($names -match '^Issue \d+$') | Sort-Object -Unique)
    if ($issueRows.Count -ne 1 -or $issueText -notmatch 'Untraversed links' -or
        $issueText -notmatch 'Link traversal' -or $issueText -notmatch 'The link target was skipped' -or
        -not ($names -match '(^|[\\/])redirect$')) {
      throw "Expected one untraversed junction issue, observed: $issueText"
    }
    Invoke-UI (Wait-Enabled 'Hide scan issues' $true)
    'F01–F03 native NTFS junction: 19-byte lower bound, one untraversed link, outside target excluded'
  } finally {
    # Remove only the junction entry before the fixture is cleaned up.
    [IO.Directory]::Delete($junction)
  }
}
# A reviewed path must not admit a replacement directory with a different identity.
function Test-ReplacedScopeRoot {
  $scope = Join-Path $root 'replaced-scope'
  $original = Join-Path $root 'reviewed-scope-original'
  [IO.Directory]::CreateDirectory($scope) | Out-Null
  [IO.File]::WriteAllBytes((Join-Path $scope 'original.bin'), [byte[]]::new(23))
  Set-Scope $scope
  Invoke-UI (Wait-Enabled 'Review scope' $true)
  $null = Wait-Enabled 'Start scan' $true

  [IO.Directory]::Move($scope, $original)
  [IO.Directory]::CreateDirectory($scope) | Out-Null
  [IO.File]::WriteAllBytes((Join-Path $scope 'replacement.bin'), [byte[]]::new(71))
  Invoke-UI (Wait-Enabled 'Start scan' $true)
  $null = Wait-UI 'replaced directory rejection' {
    Find-UI 'The target or result changed. Review the scope again.' $textType
  }
  $null = Wait-Enabled 'Start scan' $false
  if (Find-UI 'Cancel scan' $button) { throw 'The unreviewed replacement started a scan' }

  Invoke-UI (Wait-Enabled 'Review scope' $true)
  Invoke-UI (Wait-Enabled 'Start scan' $true)
  $null = Wait-FinalLogicalSize '71 bytes'
  'F01 native NTFS scope replacement: changed identity rejected; fresh review admitted the new 71-byte directory'
}
# Read the real committed scan history and return to the same pinned analysis.
function Test-OperationHistory {
  Invoke-UI (Wait-VisibleAction 'Operation history')
  $item = Wait-UI 'recorded disk analysis' {
    $window.FindAll($scopeTree, [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, $button)) |
      Where-Object { $_.Current.IsEnabled -and $_.Current.Name -like 'View details: Disk analysis,*' } |
      Select-Object -First 1
  }
  Invoke-UI $item
  $null = Wait-UI 'scope of the committed operation' { Find-UI 'metadata: 1 root' $textType }
  Invoke-UI (Wait-VisibleAction 'Back to history')
  $null = Wait-UI 'operation detail dismissed' { -not (Find-UI 'Back to history' $button) }
  Invoke-UI (Wait-VisibleAction 'Back to analysis')
  $null = Wait-FinalLogicalSize '18 bytes'
  'F21 native history: opened a committed scan and returned to the same pinned 18-byte result'
}
try {
  $window = Wait-UI 'main window' {
    $process.Refresh()
    if ($process.MainWindowHandle -ne 0) { $ae::FromHandle($process.MainWindowHandle) }
  }
  $null = Wait-UI 'path field' { Find-UI 'Windows folder path' $edit }
  $null = Wait-Enabled 'Start scan' $false
  if ($VerifyInitialDrive) { Test-InitialDrive }
  if ($VerifySettingsRestoration) {
    Invoke-UI (Wait-VisibleAction 'Settings restoration')
    $null = Wait-UI 'startup settings defaults' { Find-UI 'Using default' $textType }
    $restoredSettings = Wait-UI 'restored settings list' { Find-UI 'Restored settings' $null }
    foreach ($settingName in @('Size unit', 'Unit system', 'Default size basis')) {
      $null = Wait-UI "user-facing setting name: $settingName" { Find-UI $settingName $null $restoredSettings }
    }
    foreach ($key in @('display.size_unit', 'display.size_unit_system', 'space.default_basis')) {
      if (Find-UI $key $null $restoredSettings) { throw "Settings restoration displayed an internal key: $key" }
    }
    Wait-Details 'Format details' 'App format version'
    $null = Wait-UI 'startup settings format' { Find-UI 'App format version' $null }
    Invoke-UI (Wait-VisibleAction 'Close details')
    $null = Wait-UI 'settings report closed' { -not (Find-UI 'App format version' $null) }
    $null = Wait-VisibleAction 'Settings restoration'
    'F14 native settings restoration: user-facing setting names, startup format and defaults visible, dialog closed'
  }
  if ($VerifyNativeFolderDrop) { Test-NativeFolderDrop }
  if ($VerifyAutomaticScopeReview) { Test-AutomaticScopeReview }
  if ($VerifyDeniedSubtree) { Test-DeniedSubtree }
  if ($VerifyJunctionBoundary) { Test-JunctionBoundary }
  if ($VerifyReplacedScopeRoot) { Test-ReplacedScopeRoot }
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
  $null = Wait-UI 'allocated size as the default result metric' {
    Find-UI 'Allocated size' ([Windows.Automation.ControlType]::Group)
  }
  $null = Wait-UI '18-byte logical total' {
    Find-LogicalSize '18 bytes'
  }
  'F02/F03 native scan: Scan finished, Result version: Final, 18 bytes'
  if ($VerifyRecentScope) { Test-RecentScopeReopen }
  if ($VerifyOperationHistory) { Test-OperationHistory }
  $null = Wait-VisibleAction 'Analyze again'
  $null = Wait-UI 'target editor collapsed after accepted scan' { -not (Find-UI 'Windows folder path' $edit) }
  Set-Scope $child
  Invoke-UI (Wait-VisibleAction 'Review scope')
  $null = Wait-Enabled 'Start scan' $true
  Invoke-UI (Wait-VisibleAction 'Cancel selection')
  $null = Wait-UI 'discarded scope and retained final total' {
    $field = Find-UI 'Windows folder path' $edit
    if ($field -and (Find-UI 'Final' $textType) -and
        $field.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).Current.Value -eq '') {
      Find-LogicalSize '18 bytes'
    }
  }
  $null = Wait-Enabled 'Start scan' $false
  'F01 native selection cancellation: discarded reviewed scope and retained the final 18-byte result'
  Invoke-UI (Wait-VisibleAction 'Close target editor')
  $null = Wait-UI 'target editor closed with the result still visible' {
    if (-not (Find-UI 'Windows folder path' $edit)) {
      Find-LogicalSize '18 bytes'
    }
  }
  'F01–F03 native reanalysis: opened and closed target editing without replacing the pinned result'
  Invoke-UI (Wait-Enabled 'Explain volume usage' $true)
  $null = Wait-UI 'volume explanation for the selected root' {
    Find-UI 'Selected scanned folder' $textType
  }
  Wait-Details 'Observation and difference details' 'Scoped allocated size'
  $null = Wait-UI 'scoped allocated size' { Find-UI 'Scoped allocated size' $null }
  $null = Wait-UI 'scoped unique physical size' { Find-UI 'Scoped unique physical size' $null }
  Invoke-UI (Wait-Enabled 'Hide volume explanation' $true)
  $null = Wait-UI 'closed volume explanation' { -not (Find-UI 'Selected scanned folder' $textType) }
  'F03 native volume explanation: scoped allocation and unique physical evidence, then closed'
  Invoke-UI (Wait-Enabled 'View folder details' $true)
  Wait-Details 'Space measurement details' 'Observed item space'
  $null = Wait-UI 'pinned folder space detail' {
    $details = Find-UI 'Observed item space' $null
    if ($details) { Find-UI 'Observed logical size' $null $details }
  }
  Invoke-UI (Wait-Enabled 'Close details' $true)
  $null = Wait-UI 'closed pinned folder details' { -not (Find-UI 'Observed item space' $null) }
  'F03 native item space: opened and closed the pinned folder observations'
  Invoke-UI (Wait-Enabled 'Browse folders' $true)
  $null = Wait-UI 'root.bin' { Find-UI 'root.bin' $null }
  $null = Wait-UI '11-byte root row' { Find-UI '11 bytes' $null }
  Test-FileInspection 'root.bin' '11 bytes' $VerifyPathCopyContention
  if ($VerifyPathCopyContention) { Test-BrowseCopyContention 'root.bin' }
  Test-BrowseFolderInspection 'nested'
  $nested = Wait-UI 'nested folder button' { Find-UI 'nested' $button }
  Invoke-UI $nested
  $null = Wait-UI 'nested.bin' { Find-UI 'nested.bin' $null }
  $null = Wait-UI '7-byte nested row' { Find-UI '7 bytes' $null }
  Invoke-UI (Wait-Enabled 'Up' $true)
  $null = Wait-UI 'returned to pinned folder' { Find-UI 'root.bin' $null }
  'F05 native browse: root.bin 11 bytes, nested.bin 7 bytes, Up returns to the pinned scope'
  if ($VerifyPopulatedWorkspace -or $VerifyInspectionPathDisclosure) { Test-PopulatedWorkspace }
  Test-ScaledFileInspection
  if ($VerifyPhysicalAccounting) { Test-PhysicalAccounting }
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
function Remove-SmokeFixture {
  [CmdletBinding()]
  param([Parameter(Mandatory)][string]$FixtureRoot)

  $temp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([IO.Path]::DirectorySeparatorChar)
  $verifiedRoot = (Resolve-Path -LiteralPath $FixtureRoot).Path
  if ([IO.Path]::GetDirectoryName($verifiedRoot) -ne $temp -or
      [IO.Path]::GetFileName($verifiedRoot) -notmatch '^diskunfold-smoke-[0-9a-f-]{36}$') {
    throw "Unexpected fixture directory: $verifiedRoot"
  }
  # WebView2 subprocesses can retain profile files briefly after the launcher exits.
  # Remove the owned profile first so a lingering lock cannot partially consume the
  # fixture and its logs while the recursive cleanup retries.
  $verifiedProfile = (Resolve-Path -LiteralPath (Join-Path $verifiedRoot 'profile')).Path
  $profileParent = [IO.Path]::GetDirectoryName($verifiedProfile).TrimEnd([IO.Path]::DirectorySeparatorChar)
  if (-not [string]::Equals($profileParent, $verifiedRoot.TrimEnd([IO.Path]::DirectorySeparatorChar), [StringComparison]::OrdinalIgnoreCase) -or
      [IO.Path]::GetFileName($verifiedProfile) -cne 'profile') {
    throw "Unexpected smoke profile directory: $verifiedProfile"
  }
  $cleanupTimeoutSeconds = 30
  $cleanupDeadline = [DateTime]::UtcNow.AddSeconds($cleanupTimeoutSeconds)
  while (Test-Path -LiteralPath $verifiedProfile) {
    $cleanupFailure = $null
    $windowsError = $null
    try {
      Remove-Item -LiteralPath $verifiedProfile -Recurse -Force -ErrorAction Stop
    } catch {
      $cleanupFailure = $_
      $sharingException = $null
      $exception = $cleanupFailure.Exception
      while ($exception) {
        if ($exception -is [IO.IOException]) {
          $sharingException = $exception
          break
        }
        $exception = $exception.InnerException
      }
      if (-not $sharingException) { throw }
      $windowsError = [int]($sharingException.HResult -band 0xffff)
      if ($windowsError -notin @(32, 33)) { throw }
    }
    if (Test-Path -LiteralPath $verifiedProfile) {
      if ([DateTime]::UtcNow -ge $cleanupDeadline) {
        $failureDetails = 'The last removal attempt returned without deleting the profile.'
        if ($cleanupFailure) {
          $errorId = $cleanupFailure.FullyQualifiedErrorId
          $errorType = $cleanupFailure.Exception.GetType().FullName
          $failureDetails = "Last error id '$errorId', type '$errorType', Windows error ${windowsError}:`n$($cleanupFailure.Exception.ToString())"
        }
        throw "Timed out waiting for WebView2 to release the smoke profile after $cleanupTimeoutSeconds seconds. The isolated fixture and logs are retained at '$verifiedRoot'. $failureDetails"
      }
      Start-Sleep -Milliseconds 200
    }
  }
  Remove-Item -LiteralPath $verifiedRoot -Recurse -Force -ErrorAction Stop
}
Remove-SmokeFixture -FixtureRoot $root

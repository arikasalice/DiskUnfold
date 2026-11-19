#Requires -Version 7.4
# Runs the installed Windows UI Automation client against one real WebView2 window.
# Use a GUI build and an interactive Windows session with WebView2 installed.
[CmdletBinding()]
param(
  [Parameter(Mandatory)][string]$Binary,
  # Folder for the review captures (PNG). Defaults to a folder under %TEMP%.
  [string]$CaptureDirectory,
  # Leave the isolated fixture in %TEMP% instead of removing it. For disposable CI runners, where
  # WebView2 can still hold profile files when the cleanup runs.
  [switch]$KeepFixture,
  # Display language of the isolated profile. ko is written into the profile's settings database
  # before the app starts, so the app reads it the way it reads a saved choice.
  [ValidateSet('en', 'ko')][string]$Locale = 'en',
  # dark makes the WebView report prefers-color-scheme: dark; the app follows it.
  [ValidateSet('light', 'dark')][string]$Theme = 'light',
  # The app scale (a11y.app_scale), a CSS zoom of the whole page; 100 leaves the setting alone.
  [ValidateSet('100', '125', '150', '200')][string]$AppScale = '100',
  # Outer window size as WIDTHxHEIGHT, for example 1280x800. The screen must be at least this large.
  [string]$WindowSize,
  # Walk the main screens once and capture each one, without the value checks of the full flow.
  [switch]$CaptureTour,
  # Capture a depth 2 treemap of a folder of known files and check the names inside a folder.
  [switch]$CaptureTreemap,
  # Capture the fallback table of an unavailable chart, and fold and unfold its title with the
  # mouse, Enter and Space.
  [switch]$CaptureFallbackTable,
  # Capture a depth 2 treemap of a scope with an NTFS junction.
  [switch]$CaptureTreemapLink,
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
  [switch]$VerifyLocaleSwitch,
  [switch]$VerifyKoreanIme,
  [switch]$VerifyOperationHistory,
  [switch]$VerifyInspectionPathDisclosure,
  [switch]$VerifyInspectionKeyboard,
  [switch]$VerifyPathCopyContention,
  [switch]$VerifyRecentScope,
  [switch]$VerifyNativeFolderDrop,
  [switch]$VerifyVirtualGroups,
  [switch]$VerifyVirtualGroupsProgress,
  [switch]$VerifyVirtualGroupsSample
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

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool SetCursorPos(int x, int y);

    // A real left click at a screen point, through the same input queue as a key.
    public static void Click(IntPtr window, int x, int y) {
      if (window == IntPtr.Zero || GetForegroundWindow() != window)
        throw new InvalidOperationException("DiskUnfold must own the foreground window.");
      if (!SetCursorPos(x, y))
        throw new Win32Exception(Marshal.GetLastWin32Error(), "Could not move the pointer.");
      const uint MouseType = 0;
      const uint LeftDown = 2;
      const uint LeftUp = 4;
      var inputs = new Input[2];
      inputs[0].Type = inputs[1].Type = MouseType;
      inputs[0].Data.Mouse.Flags = LeftDown;
      inputs[1].Data.Mouse.Flags = LeftUp;
      if (SendInput(2, inputs, Marshal.SizeOf<Input>()) != 2)
        throw new Win32Exception(Marshal.GetLastWin32Error(), "Could not send both mouse events.");
    }

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
# The UIA names in this script are English. For another display language they are mapped through the
# message bundles of the repository (the same text the screens show).
function Initialize-UiNames([string]$language) {
  $directory = Join-Path $PSScriptRoot '..\..\user-env\i18n'
  $read = {
    param($file)
    $entries = [ordered]@{}
    foreach ($line in Get-Content -LiteralPath (Join-Path $directory $file)) {
      if ($line -match '^\s*\["([\w.]+)",\s*("(?:[^"\\]|\\.)*"),') { $entries[$Matches[1]] = ($Matches[2] | ConvertFrom-Json) }
    }
    $entries
  }
  $english = & $read 'en.json'
  $target = & $read "$language.json"
  $script:UiExact = @{}
  $script:UiTemplates = @()
  foreach ($key in $english.Keys) {
    if (-not $target.Contains($key)) { continue }
    $source = $english[$key]
    if ($source -match '\{\w+\}') {
      try {
        $pattern = '^' + ([regex]::Escape($source) -replace '\\\{(\w+)\}', '(?<$1>.+?)') + '$'
        $script:UiTemplates += [pscustomobject]@{ Pattern = [regex]::new($pattern); Result = $target[$key] }
      } catch { continue }
    } elseif (-not $script:UiExact.ContainsKey($source)) { $script:UiExact[$source] = $target[$key] }
  }
}
function Get-UiName([string]$name) {
  if ($null -eq $script:UiExact) { return $name }
  if ($script:UiExact.ContainsKey($name)) { return $script:UiExact[$name] }
  foreach ($template in $script:UiTemplates) {
    $match = $template.Pattern.Match($name)
    if ($match.Success) { return [regex]::Replace($template.Result, '\{(\w+)\}', { param($m) $match.Groups[$m.Groups[1].Value].Value }) }
  }
  $name
}
# The settings database exists only after a first start: start the app once, close it, then store the
# display language in it with python's sqlite3 and let the real start read it.
function Initialize-SmokeLocale([string]$language, [string]$scale = '100') {
  $python = Get-Command python -ErrorAction SilentlyContinue
  if (-not $python) { throw 'python is needed to store the display language in the isolated profile' }
  $database = Join-Path $profile 'DiskUnfold\diskunfold.db'
  $first = Start-Process -FilePath $exe -PassThru -Environment $appEnvironment
  try {
    $deadline = [DateTime]::UtcNow.AddSeconds(60)
    while ($true) {
      $first.Refresh()
      if ($first.HasExited) { throw 'DiskUnfold exited before it created its settings database' }
      if ((Test-Path -LiteralPath $database) -and $first.MainWindowHandle -ne 0) { break }
      if ([DateTime]::UtcNow -gt $deadline) { throw 'Timed out waiting for the settings database' }
      Start-Sleep -Milliseconds 200
    }
    Start-Sleep -Seconds 2
  } finally {
    if (-not $first.HasExited) {
      $null = $first.CloseMainWindow()
      if (-not $first.WaitForExit(15000)) { Stop-Process -Id $first.Id -Force; $first.WaitForExit() }
    }
  }
  $seed = @'
import sqlite3, sys
connection = sqlite3.connect(sys.argv[1])
connection.execute("INSERT OR REPLACE INTO user_env_setting (setting_key, value_json, modified_source, created_at_ms, updated_at_ms, row_version) VALUES ('locale.display_language', ?, 'owner', 0, 0, 1)", ('"' + sys.argv[2] + '"',))
if sys.argv[3] != "100":
    connection.execute("INSERT OR REPLACE INTO user_env_setting (setting_key, value_json, modified_source, created_at_ms, updated_at_ms, row_version) VALUES ('a11y.app_scale', ?, 'owner', 0, 0, 1)", (sys.argv[3],))
connection.execute("UPDATE user_env_setting_version SET settings_version = settings_version + 1, updated_at_ms = 0 WHERE version_key = 'settings'")
connection.commit()
connection.close()
'@
  $seed | & $python.Source - $database $language $scale
  if ($LASTEXITCODE -ne 0) { throw "Could not store the display language and app scale (python exit code $LASTEXITCODE)" }
}
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class DiskUnfoldSmokeWindow {
  [DllImport("user32.dll")]
  private static extern bool SetWindowPos(IntPtr window, IntPtr after, int x, int y, int width, int height, uint flags);
  // SWP_NOZORDER | SWP_NOACTIVATE
  public static bool Place(IntPtr window, int width, int height) { return SetWindowPos(window, IntPtr.Zero, 0, 0, width, height, 0x0014); }
}
'@
# Put the window at the top left corner with the requested outer size: UI Automation first, then the
# window API when the window ignores it. The measured size is printed.
function Set-SmokeWindowSize([string]$size) {
  if ($size -notmatch '^(\d+)x(\d+)$') { throw "Invalid -WindowSize '$size'; use WIDTHxHEIGHT" }
  $width = [int]$Matches[1]
  $height = [int]$Matches[2]
  $measure = {
    $bounds = $window.Current.BoundingRectangle
    [pscustomobject]@{ Text = "$([int]$bounds.Width)x$([int]$bounds.Height)"; Fits = ([Math]::Abs($bounds.Width - $width) -le 20 -and [Math]::Abs($bounds.Height - $height) -le 20) }
  }
  $windowPattern = $window.GetCurrentPattern([Windows.Automation.WindowPattern]::Pattern)
  $windowPattern.SetWindowVisualState([Windows.Automation.WindowVisualState]::Normal)
  $transform = $window.GetCurrentPattern([Windows.Automation.TransformPattern]::Pattern)
  "Window size: before $((& $measure).Text); UI Automation can resize: $($transform.Current.CanResize)"
  $transform.Move(0, 0)
  $transform.Resize($width, $height)
  Start-Sleep -Milliseconds 1000
  $state = & $measure
  if (-not $state.Fits) {
    "Window size: UI Automation left $($state.Text); using SetWindowPos"
    $null = [DiskUnfoldSmokeWindow]::Place([IntPtr]$window.Current.NativeWindowHandle, $width, $height)
    Start-Sleep -Milliseconds 1000
    $state = & $measure
  }
  "Window size: requested $size, measured $($state.Text)"
  if (-not $state.Fits) { throw "The window did not take the size $size (measured $($state.Text)); the screen may be too small" }
}
if ($Locale -ne 'en') { Initialize-UiNames $Locale }
$profile = Join-Path $root 'profile'
New-Item -ItemType Directory -Path $profile | Out-Null
$appEnvironment = @{ LOCALAPPDATA = $profile; DISKUNFOLD_DATA_DIR = (Join-Path $profile 'DiskUnfold') }
if ($Locale -ne 'en' -or $AppScale -ne '100') { Initialize-SmokeLocale $Locale $AppScale }
# The WebView reports prefers-color-scheme from the Windows app color mode, which the app follows. Set
# it for this run only and put it back in the final block below.
$themeKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize'
$originalAppTheme = (Get-ItemProperty -LiteralPath $themeKey -Name AppsUseLightTheme -ErrorAction SilentlyContinue).AppsUseLightTheme
if ($Theme -eq 'dark') {
  if (-not (Test-Path -LiteralPath $themeKey)) { $null = New-Item -Path $themeKey -Force }
  Set-ItemProperty -LiteralPath $themeKey -Name AppsUseLightTheme -Type DWord -Value 0
}
$process = Start-Process -FilePath $exe -PassThru -Environment $appEnvironment -RedirectStandardError (Join-Path $root 'stderr.txt') -RedirectStandardOutput (Join-Path $root 'stdout.txt')
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
  $name = Get-UiName $name
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
  foreach ($name in @('Cancel selection', 'Review scope', 'Start analysis')) {
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
function Find-ResultVersionLabel($label, $prefix = 'Result version: ') {
  $summary = Find-UI ($prefix + $label) $textType
  if ($summary) { return $summary }
  Find-UI $label $textType
}
# The summary's "Result version: <status>" label in the profile's language; the template and the
# status are translated separately, because Get-UiName leaves a template's value as given.
function Get-ResultVersionName($status) {
  (Get-UiName 'Result version: {status}').Replace('{status}', (Get-UiName $status))
}
# Wait for the expected total, selecting a newer result if the interim one stays pinned.
function Wait-FinalLogicalSize($expected, $versionLabel = 'Final') {
  $selection = @{ LastSwitch = [DateTime]::MinValue }
  Wait-UI "final logical size $expected" {
    if (-not (Find-UI 'Scan finished' $textType)) { return }
    if (Find-ResultVersionLabel $versionLabel) {
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
    Find-UI 'View details' ([Windows.Automation.ControlType]::MenuItem)
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
  Invoke-UI (Wait-Enabled 'Start analysis' $true)
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
    Invoke-UI (Wait-Enabled 'Start analysis' $true)
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
  Invoke-UI (Wait-Enabled 'Start analysis' $true)
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
    Expand-UI (Wait-Enabled 'Recent analysis scopes' $true)
    Invoke-UI (Wait-Enabled 'Review scope 1' $true)
    $null = Wait-UI 'stored volume matched' { Find-UI 'Volume matched' $textType }
    Invoke-UI (Wait-Enabled 'Use this scope' $true)
    $null = Wait-UI 'visible focused replacement in small window' {
      $focused = $ae::FocusedElement
      if ($focused.Current.Name -eq 'Replace current scope' -and -not $focused.Current.IsOffscreen) { $focused }
    }
    Invoke-UI (Wait-Enabled 'Keep editing' $true)
    $use = Wait-UI 'visible focused apply action after keeping edits' {
      $focused = $ae::FocusedElement
      if ($focused.Current.Name -eq 'Use this scope' -and -not $focused.Current.IsOffscreen) { $focused }
    }
    Invoke-UI $use
    $confirm = Wait-Enabled 'Replace current scope' $true
    Invoke-UI $confirm
    'F01 native recent scope: confirmation and return actions keep visible focus in the small window'
  } finally {
    $transform.Resize($original.Width, $original.Height)
  }
  $null = Wait-Enabled 'Start analysis' $true
  if (-not (Find-LogicalSize '18 bytes')) { throw 'Applying the saved scope replaced the pinned result' }
  [IO.File]::WriteAllBytes((Join-Path $scanRoot 'root.bin'), [byte[]]::new(16))
  Invoke-UI (Wait-Enabled 'Start analysis' $true)
  $null = Wait-FinalLogicalSize '23 bytes'
  'F01 native recent scope: matched saved volume, preserved pinned 18-byte result until explicit scan, then read 23 bytes'

  [IO.File]::WriteAllBytes((Join-Path $scanRoot 'root.bin'), [byte[]]::new(11))
  Set-Scope $child
  Set-Scope $scanRoot
  Invoke-UI (Wait-Enabled 'Review scope' $true)
  Invoke-UI (Wait-Enabled 'Start analysis' $true)
  $null = Wait-FinalLogicalSize '18 bytes'
}
# The operator performs the OLE gesture; UIA only observes its result.
function Test-NativeFolderDrop {
  Set-Scope $scanRoot
  Invoke-UI (Wait-Enabled 'Review scope' $true)
  $null = Wait-Enabled 'Start analysis' $true
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
  $null = Wait-Enabled 'Start analysis' $false
  Invoke-UI (Wait-Enabled 'Review scope' $true)
  $null = Wait-Enabled 'Start analysis' $true
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
  $null = Wait-Enabled 'Start analysis' $true
  Wait-ScopeActionsVisible
  Invoke-UI (Wait-Enabled 'Start analysis' $true)
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
  Invoke-UI (Wait-Enabled 'Start analysis' $true)
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
  Invoke-UI (Wait-Enabled 'Start analysis' $true)
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
    $null = Wait-Enabled 'Start analysis' $false
    Invoke-UI (Wait-VisibleAction 'Review scope')
    $null = Wait-VisibleAction 'Start analysis'
    Invoke-UI (Wait-VisibleAction 'Cancel selection')
    $null = Wait-Enabled 'Start analysis' $false
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
    Invoke-UI (Wait-Enabled 'Start analysis' $true)
    $null = Wait-FinalLogicalSize 'At least 13 bytes' 'Final with scan gaps'
    Invoke-UI (Wait-Enabled 'View scan issues' $true)
    $null = Wait-UI 'access denied evidence' {
      $issues = Find-UI 'Scan issue entries' $null
      if ($issues) {
        $names = $issues.FindAll($scopeTree, [Windows.Automation.Condition]::TrueCondition) |
          ForEach-Object { $_.Current.Name }
        ($names -contains 'Unread locations') -and ($names -contains 'Opening a folder') -and
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
    Invoke-UI (Wait-Enabled 'Start analysis' $true)
    # The scan is final even though the link limits measurement completeness.
    $null = Wait-FinalLogicalSize 'At least 19 bytes'
    Invoke-UI (Wait-Enabled 'View scan issues' $true)
    $issues = Wait-UI 'junction issue list' { Find-UI 'Scan issue entries' $null }
    $names = @($issues.FindAll($scopeTree, [Windows.Automation.Condition]::TrueCondition) |
      ForEach-Object { $_.Current.Name } | Where-Object { $_ })
    $issueText = $names -join ' '
    $issueRows = @(($names -match '^Issue \d+$') | Sort-Object -Unique)
    if ($issueRows.Count -ne 1 -or $issueText -notmatch 'Links not followed' -or
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
  $null = Wait-Enabled 'Start analysis' $true

  [IO.Directory]::Move($scope, $original)
  [IO.Directory]::CreateDirectory($scope) | Out-Null
  [IO.File]::WriteAllBytes((Join-Path $scope 'replacement.bin'), [byte[]]::new(71))
  Invoke-UI (Wait-Enabled 'Start analysis' $true)
  $null = Wait-UI 'replaced directory rejection' {
    Find-UI 'The target or result changed. Review the scope again.' $textType
  }
  $null = Wait-Enabled 'Start analysis' $false
  if (Find-UI 'Cancel scan' $button) { throw 'The unreviewed replacement started a scan' }

  Invoke-UI (Wait-Enabled 'Review scope' $true)
  Invoke-UI (Wait-Enabled 'Start analysis' $true)
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
function Select-SmokeLanguage($settingsName, $fieldName, $languageName, $nextFieldName, $closeName) {
  Invoke-UI (Wait-VisibleAction $settingsName)
  $field = Wait-UI $fieldName { Find-UI $fieldName ([Windows.Automation.ControlType]::ComboBox) }
  $field.SetFocus()
  $null = Wait-UI "focused $fieldName" { $field.Current.HasKeyboardFocus }
  # Home and End change the focused native select without relying on its popup UIA state.
  if ($languageName -eq 'English') { Send-SmokeKey 0x24 } else { Send-SmokeKey 0x23 }
  $null = Wait-UI $nextFieldName { Find-UI $nextFieldName ([Windows.Automation.ControlType]::ComboBox) }
  Invoke-UI (Wait-VisibleAction $closeName)
}
function Enter-KoreanImeText($field, $prefix) {
  $field.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).SetValue($prefix)
  $field.SetFocus()
  $null = Wait-UI 'focused Korean path input' { $field.Current.HasKeyboardFocus }
  Send-SmokeKey 0x23
  Send-SmokeKey 0x52
  Send-SmokeKey 0x4B
  Send-SmokeKey 0x0D
}
function Test-KoreanIme($field, $directory) {
  $expected = Join-Path $directory '가'
  New-Item -ItemType Directory -Path $expected | Out-Null
  $prefix = $directory.TrimEnd('\') + '\'
  $toggled = $false
  try {
    Enter-KoreanImeText $field $prefix
    $inputMode = Wait-UI 'Korean composition or Latin input' {
      $value = $field.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).Current.Value
      if ($value -eq $expected) { 'korean' }
      elseif ($value -eq ($prefix + 'rk')) { 'latin' }
    }
    if ($inputMode -eq 'latin') {
      Send-SmokeKey 0x15
      $toggled = $true
      Enter-KoreanImeText $field $prefix
    }
    $imeState = [pscustomobject]@{ LastValue = $null }
    try {
      $null = Wait-UI 'committed Korean IME composition' {
        $imeState.LastValue = $field.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).Current.Value
        $imeState.LastValue -eq $expected
      }
    } catch {
      $focusedName = try { $ae::FocusedElement.Current.Name } catch { '<unavailable>' }
      throw "Korean IME did not commit the expected path '$expected'; last UIA value '$($imeState.LastValue)', focused element '$focusedName'. $($_.Exception.Message)"
    }
    if (Find-UI '스캔 진행 상황' $null) { throw 'IME confirmation started a scan' }
    'F13 native Korean IME: Enter committed the composed path without starting a scan'
  } finally {
    if ($toggled) { Send-SmokeKey 0x15 }
  }
}
function Test-LocaleSwitch {
  $field = Wait-UI 'localized path field' {
    $inputField = Find-UI 'Windows folder path' $edit
    if (-not $inputField) { $inputField = Find-UI 'Windows 폴더 경로' $edit }
    $inputField
  }
  $value = $field.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern)
  $draftPath = Join-Path $root 'locale-scope'
  New-Item -ItemType Directory -Path $draftPath | Out-Null
  $value.SetValue($draftPath)
  if ($field.Current.Name -eq 'Windows folder path') {
    Select-SmokeLanguage 'Language and display' 'Display language' '한국어' '표시 언어' '닫기'
  } else {
    Select-SmokeLanguage '언어 및 표시' '표시 언어' 'English' 'Display language' 'Close'
    Select-SmokeLanguage 'Language and display' 'Display language' '한국어' '표시 언어' '닫기'
  }
  $localized = Wait-UI 'Korean path field' { Find-UI 'Windows 폴더 경로' $edit }
  if ($localized.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).Current.Value -ne $draftPath) {
    throw 'Locale switch discarded the current scope draft'
  }
  if ($VerifyKoreanIme) { Test-KoreanIme $localized $draftPath }
  Select-SmokeLanguage '언어 및 표시' '표시 언어' 'English' 'Display language' 'Close'
  $english = Wait-UI 'English path field' { Find-UI 'Windows folder path' $edit }
  $english.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).SetValue('')
  'F13 native locale: switched between Korean and English without discarding the scope draft'
}
# Save the window as a PNG under %TEMP% when the interactive desktop allows it.
function Save-SmokeCapture($name) {
  $restore = $null
  try {
    # Review captures show the whole window at the screen's full size; the size is restored
    # afterwards so later steps see the layout they were written for.
    $windowPattern = $window.GetCurrentPattern([Windows.Automation.WindowPattern]::Pattern)
    if (-not $WindowSize -and $windowPattern.Current.WindowVisualState -ne [Windows.Automation.WindowVisualState]::Maximized) {
      $restore = $windowPattern.Current.WindowVisualState
      $windowPattern.SetWindowVisualState([Windows.Automation.WindowVisualState]::Maximized)
      Start-Sleep -Milliseconds 800
    }
    # Let entrance transitions (dialogs, panels) finish painting before the pixels are copied.
    Start-Sleep -Milliseconds 700
    Add-Type -AssemblyName System.Drawing
    $bounds = $window.Current.BoundingRectangle
    $directory = if ($CaptureDirectory) { $CaptureDirectory } else { Join-Path ([IO.Path]::GetTempPath()) 'diskunfold-virtual-groups-captures' }
    [IO.Directory]::CreateDirectory($directory) | Out-Null
    $bitmap = [Drawing.Bitmap]::new([int]$bounds.Width, [int]$bounds.Height)
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    try {
      $graphics.CopyFromScreen([int]$bounds.X, [int]$bounds.Y, 0, 0, $bitmap.Size)
      $path = Join-Path $directory "$name.png"
      $bitmap.Save($path, [Drawing.Imaging.ImageFormat]::Png)
    } finally { $graphics.Dispose(); $bitmap.Dispose() }
    "Capture: $path ($([int]$bounds.Width)x$([int]$bounds.Height))"
  } catch { Write-Warning "Capture '$name' failed: $($_.Exception.Message)" }
  finally {
    if ($null -ne $restore) {
      try { $windowPattern.SetWindowVisualState($restore); Start-Sleep -Milliseconds 500 }
      catch { Write-Warning "Restoring the window after '$name' failed: $($_.Exception.Message)" }
    }
  }
}
function Get-UINames($within = $window) {
  @($within.FindAll($scopeTree, [Windows.Automation.Condition]::TrueCondition) |
    ForEach-Object { $_.Current.Name } | Where-Object { $_ })
}
# Write the control types and names of the whole window, to adapt the names on failure.
function Write-UITree($label) {
  $lines = $window.FindAll($scopeTree, [Windows.Automation.Condition]::TrueCondition) | ForEach-Object {
    '{0}|{1}|enabled={2}|offscreen={3}' -f $_.Current.ControlType.ProgrammaticName, $_.Current.Name, $_.Current.IsEnabled, $_.Current.IsOffscreen
  }
  $path = Join-Path $root "uia-tree-$label.txt"
  Set-Content -LiteralPath $path -Value $lines -Encoding UTF8
  $path
}
# An input's accessible name can carry its description, so fall back to a prefix match.
function Find-EditByLabel($fieldName) {
  $exact = Find-UI $fieldName $edit
  if ($exact) { return $exact }
  $window.FindAll($scopeTree, [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, $edit)) |
    Where-Object { $_.Current.Name.StartsWith($fieldName) } | Select-Object -First 1
}
# Wait-UI owns a $label parameter, so these helpers use $fieldName inside their probes.
function Set-GroupField($fieldName, $value) {
  # A field re-rendered between lookup and input can fail SetValue once; find it again.
  for ($attempt = 1; ; $attempt++) {
    $field = Wait-UI "field $fieldName" { Find-EditByLabel $fieldName }
    try { $field.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).SetValue($value); break }
    catch { if ($attempt -ge 3) { throw }; Start-Sleep -Milliseconds 500 }
  }
  $null = Wait-UI "value of $fieldName" {
    $current = Find-EditByLabel $fieldName
    $current -and $current.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).Current.Value -ceq $value
  }
}
# The closed native select moves by key press, as in Select-SmokeLanguage.
function Select-GroupOption($fieldName, $key, $expected) {
  $combo = Wait-UI "selector $fieldName" { Find-UI $fieldName ([Windows.Automation.ControlType]::ComboBox) }
  $combo.SetFocus()
  $null = Wait-UI "focused $fieldName" {
    $current = Find-UI $fieldName ([Windows.Automation.ControlType]::ComboBox)
    $current -and $current.Current.HasKeyboardFocus
  }
  Send-SmokeKey $key
  $null = Wait-UI "$fieldName = $expected" {
    $current = Find-UI $fieldName ([Windows.Automation.ControlType]::ComboBox)
    $pattern = $null
    $current -and $current.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern, [ref]$pattern) -and
      $pattern.Current.Value -ceq $expected
  }
}
# Files keep their size and write time; folders are compared by name only, because NTFS may
# update a folder's write time shortly after a file inside it was created.
function Get-FixtureState($path) {
  Get-ChildItem -LiteralPath $path -Recurse -Force | Sort-Object FullName | ForEach-Object {
    if ($_.PSIsContainer) { '{0}|dir' -f $_.FullName.Substring($path.Length) }
    else { '{0}|{1}|{2}' -f $_.FullName.Substring($path.Length), $_.Length, $_.LastWriteTimeUtc.Ticks }
  }
}
# Close the app and open it again on the same isolated data folder.
function Restart-SmokeApplication {
  $process.Refresh()
  $null = $process.CloseMainWindow()
  if (-not $process.WaitForExit(10000)) { Stop-Process -Id $process.Id; throw 'The test process did not exit before restart' }
  if ($process.ExitCode -ne 0 -or (Get-Item (Join-Path $root 'stderr.txt')).Length -ne 0) { throw "Abnormal exit before restart, log at $root" }
  Start-Sleep -Seconds 3
  $script:process = Start-Process -FilePath $exe -PassThru -Environment @{ LOCALAPPDATA = $profile; DISKUNFOLD_DATA_DIR = (Join-Path $profile 'DiskUnfold') } -RedirectStandardError (Join-Path $root 'stderr-restart.txt') -RedirectStandardOutput (Join-Path $root 'stdout-restart.txt')
  $script:window = Wait-UI 'restarted main window' {
    $process.Refresh()
    if ($process.MainWindowHandle -ne 0) { $ae::FromHandle($process.MainWindowHandle) }
  }
  if ($WindowSize) { Set-SmokeWindowSize $WindowSize }
  $null = Wait-UI 'path field after restart' { Find-UI 'Windows folder path' $edit }
}
# Scan the fixture, open the browse screen and return its virtual-group entry button.
# Without $total the scan is only waited for until it has finished (large fixtures).
function Open-VirtualGroups($path, $total) {
  Set-Scope $path
  Invoke-UI (Wait-Enabled 'Review scope' $true)
  # A busy WebView can fail the UIA invoke of a large scan's start although the scan began.
  try { Invoke-UI (Wait-Enabled 'Start analysis' $true) }
  catch {
    $failure = $_
    try { $null = Wait-UI 'scan running after a failed start invoke' { Find-UI 'Pause scan' $button } 30 }
    catch { throw $failure }
    Write-Warning "Start analysis reported '$($failure.Exception.Message)' but the scan is running"
  }
  if ($total) { $null = Wait-FinalLogicalSize $total }
  else {
    # A large scan can keep the interim result on screen until the latest one is requested.
    $null = Wait-UI 'large scan finished' {
      $latest = Find-UI 'Show latest result' $button
      if ($latest -and $latest.Current.IsEnabled) { try { Invoke-UI $latest } catch { Write-Warning "Show latest result: $($_.Exception.Message)" } }
      (Find-UI 'Scan finished' $textType) -and (Find-ResultVersionLabel 'Final')
    } 900
  }
  Invoke-UI (Wait-Enabled 'Browse folders' $true)
  Wait-UI 'virtual group entry' {
    $item = Find-UI 'View as virtual groups' $button
    if ($item -and $item.Current.IsEnabled -and -not $item.Current.IsOffscreen) { $item }
  }
}
# Count cells of the summary row that starts at the named element, up to the next row.
function Get-RowCounts($names, $start, $stops) {
  $index = [Array]::IndexOf($names, $start)
  if ($index -lt 0) { throw "Summary row '$start' was not found in: $($names -join ' / ')" }
  $end = $names.Count
  foreach ($stop in $stops) {
    $position = [Array]::IndexOf($names, $stop)
    if ($position -gt $index -and $position -lt $end) { $end = $position }
  }
  @($names[$index..($end - 1)] | Where-Object { $_ -match '^\d+ items?$' })
}
# The count a category tab shows. The categories are a tab list ("Categories", UIA Tab) of tab
# items named by their label; the count is the first number after the item's name, or part of
# the name itself ("Overlaps 2") if the host composes the name from the tab's content.
function Get-TabCount($names, $tab) {
  foreach ($candidate in $names) { if ($candidate -match ('^' + [regex]::Escape($tab) + '\s+(\d+)$')) { return [int]$Matches[1] } }
  $index = [Array]::IndexOf($names, $tab)
  if ($index -lt 0) { throw "Category '$tab' was not found in: $($names -join ' / ')" }
  foreach ($candidate in $names[($index + 1)..($names.Count - 1)]) { if ($candidate -match '^\d+$') { return [int]$candidate } }
  throw "No count follows category '$tab' in: $($names -join ' / ')"
}
# How many buttons carry one name; a control that must be drawn once is counted.
function Get-ButtonCount($buttonName) {
  $condition = [Windows.Automation.AndCondition]::new(
    [Windows.Automation.PropertyCondition]::new($ae::NameProperty, $buttonName),
    [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, $button))
  @($window.FindAll($scopeTree, $condition)).Count
}
# One pass through the main screens for design review: the same steps in any language, size and theme,
# without the value checks of the full flow. Names go through Get-UiName, so a Korean profile works.
# F06 #130: a depth 2 treemap. Shapes inside a folder take their names from that folder's rows
# and are named with it, and 400 small files in one folder, each below the minimum shape area,
# become "Other (400)" there. Tiny is too small for a header band (README 27) and stays one shape.
# The large files keep each small one under the minimum area at 1280x800 and 700x800 (about 11 px²
# of 16 inside Photos at 1280x800), while the 400 of them make a group wide enough to see.
# The group's lower-bound and unknown states need unreadable members and stay with the GUI tests.
function Test-CaptureTreemap {
  $scope = Join-Path $root 'treemap-scope'
  $photos = Join-Path $scope 'Photos'
  $videos = Join-Path $scope 'Videos'
  $tiny = Join-Path $scope 'Tiny'
  [IO.Directory]::CreateDirectory($photos) | Out-Null
  [IO.Directory]::CreateDirectory($videos) | Out-Null
  [IO.Directory]::CreateDirectory($tiny) | Out-Null
  [IO.File]::WriteAllBytes((Join-Path $photos 'album-a.bin'), [byte[]]::new(64MB))
  [IO.File]::WriteAllBytes((Join-Path $photos 'album-b.bin'), [byte[]]::new(48MB))
  for ($index = 0; $index -lt 400; $index++) {
    [IO.File]::WriteAllBytes((Join-Path $photos "small-$index.bin"), [byte[]]::new(8000))
  }
  [IO.File]::WriteAllBytes((Join-Path $videos 'clip.bin'), [byte[]]::new(128MB))
  [IO.File]::WriteAllBytes((Join-Path $scope 'notes.bin'), [byte[]]::new(16MB))
  [IO.File]::WriteAllBytes((Join-Path $tiny 'tiny-a.bin'), [byte[]]::new(512KB))
  [IO.File]::WriteAllBytes((Join-Path $tiny 'tiny-b.bin'), [byte[]]::new(512KB))
  Set-Scope $scope
  Invoke-UI (Wait-Enabled 'Review scope' $true)
  Invoke-UI (Wait-Enabled 'Start analysis' $true)
  $null = Wait-UI 'Scan finished' { Find-UI 'Scan finished' $textType } 60
  # #155: right after the scan finishes the summary may still show an earlier result. Browse
  # folders opens the scan's newest result, which the engine accepts; an earlier one it refuses.
  $summary = if (Find-UI (Get-ResultVersionName 'Final') $textType) { 'final' } else { 'earlier' }
  Invoke-UI (Wait-Enabled 'Browse folders' $true)
  "Browse folders pressed while the summary showed the $summary result"
  $null = Wait-UI 'Photos' {
    if (Find-UI 'Could not open this folder.' $null) {
      throw "Browse could not open the first folder; the summary showed the $summary result when Browse folders was pressed"
    }
    Find-UI 'Photos' $null
  }
  Invoke-UI (Wait-Enabled 'Show space distribution' $true)
  $group = Get-UiName 'Other (400)'
  $null = Wait-UI 'Photos and the named shapes inside it' {
    $names = Get-UINames
    ($names | Where-Object { $_ -like 'Photos, *' }) -and
    ($names | Where-Object { $_ -like 'album-a.bin, *Photos*' }) -and
    ($names | Where-Object { $_ -like "$group, *Photos*" }) -and
    ($names | Where-Object { $_ -like 'Tiny · *' })
  } 60
  if (Get-UINames | Where-Object { $_ -like 'tiny-*' }) { throw 'Tiny was expanded below its header band minimum' }
  Save-SmokeCapture '01-treemap-depth2'
  "Treemap depth 2: Photos holds album-a.bin and $group, Tiny stays one shape; $Locale, $Theme, window $(if ($WindowSize) { $WindowSize } else { 'default' })"
}
# F06 #134: a treemap of a scope with an NTFS junction. The Windows collector does not report the
# junction's own size, so this is the unknown-size case: the folder holding it is drawn by its lower
# bound (not refused), the file beside the junction is a named shape, and the junction has no area.
function Test-CaptureTreemapLink {
  $fixture = Join-Path $root 'treemap-link'
  $scope = Join-Path $fixture 'scope'
  $docs = Join-Path $scope 'Docs'
  $outside = Join-Path $fixture 'outside'
  [IO.Directory]::CreateDirectory($docs) | Out-Null
  [IO.Directory]::CreateDirectory($outside) | Out-Null
  [IO.File]::WriteAllBytes((Join-Path $docs 'report.bin'), [byte[]]::new(32MB))
  [IO.File]::WriteAllBytes((Join-Path $scope 'media.bin'), [byte[]]::new(64MB))
  [IO.File]::WriteAllBytes((Join-Path $outside 'outside-marker.bin'), [byte[]]::new(1MB))
  Push-Location -LiteralPath $fixture
  try {
    & cmd.exe /D /C mklink /J 'scope\Docs\redirect' outside | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Could not create the isolated NTFS junction' }
  } finally { Pop-Location }
  try {
    Set-Scope $scope
    Invoke-UI (Wait-Enabled 'Review scope' $true)
    Invoke-UI (Wait-Enabled 'Start analysis' $true)
    $null = Wait-UI 'Scan finished' { Find-UI 'Scan finished' $textType } 60
    Invoke-UI (Wait-Enabled 'Browse folders' $true)
    $null = Wait-UI 'Docs' { Find-UI 'Docs' $null }
    Invoke-UI (Wait-Enabled 'Show space distribution' $true)
    $null = Wait-UI 'Docs and the file inside it' {
      $names = Get-UINames
      ($names | Where-Object { $_ -like 'Docs, *' }) -and ($names | Where-Object { $_ -like 'report.bin, *Docs*' })
    } 60
    if (Get-UINames | Where-Object { $_ -like 'redirect, *' }) { throw 'The junction was given an area' }
    # #163: once the chart is shown it is not requested again and again. Each time the chart
    # loading row appears is a new request; it may come once more while the chart settles.
    $loading = Get-UiName 'Loading chart'
    $progress = [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, [Windows.Automation.ControlType]::ProgressBar)
    $requests = 0
    $lastSeen = $null
    $shown = $false
    $started = [DateTime]::UtcNow
    while ([DateTime]::UtcNow -lt $started.AddSeconds(6)) {
      $now = @($window.FindAll($scopeTree, $progress) | Where-Object { $_.Current.Name -eq $loading }).Count -gt 0
      if ($now -and -not $shown) { $requests++; $lastSeen = [DateTime]::UtcNow }
      $shown = $now
      Start-Sleep -Milliseconds 100
    }
    "Chart requests after it was shown: $requests"
    if ($shown -or $requests -gt 2 -or ($lastSeen -and $lastSeen -gt $started.AddSeconds(3))) {
      throw "The chart kept being requested: $requests loading rows in 6 s, still loading: $shown"
    }
    Save-SmokeCapture '01-treemap-link'
    # README 32: at 150% and above the head and the banner scroll with the page. Focusing the
    # chart's region brings it to the top of the window with its button row; the head then lies above.
    if ([int]$AppScale -ge 150) {
      $chart = Wait-UI 'chart region' { Find-UI 'Storage chart' ([Windows.Automation.ControlType]::Group) }
      $chart.SetFocus()
      Start-Sleep -Milliseconds 800
      $head = Find-UI 'Language and display' $button
      "Page scrolled to the chart buttons; the head is $(if ($head -and $head.Current.IsOffscreen) { 'above the window' } else { 'still in view' })"
      Save-SmokeCapture '02-treemap-link-scrolled'
    }
    "Treemap with a junction: Docs is drawn with report.bin inside it, the junction has no area; $Locale, $Theme, window $(if ($WindowSize) { $WindowSize } else { 'default' })"
  } finally {
    [IO.Directory]::Delete((Join-Path $docs 'redirect'))
  }
}
# F06 #129 item 6: an unavailable chart opens the folder's table under its notice (#109), and the
# table title must fold and unfold with the mouse, Enter and Space and say so (aria-expanded).
# Every unavailable reason uses the same table. A runner cannot produce no_attribution: folders
# that share a hard link are counted as items not drawn. On the unique physical size basis a scan
# never proves that extents are unshared, so no item has a size and the chart is unavailable with
# no_item_sizes, which shows the same table.
function Get-SmokeExpanded($item) {
  $disclosure = $null
  if (-not $item.TryGetCurrentPattern([Windows.Automation.ExpandCollapsePattern]::Pattern, [ref]$disclosure)) {
    throw "'$($item.Current.Name)' exposes no aria-expanded state"
  }
  $disclosure.Current.ExpandCollapseState -eq [Windows.Automation.ExpandCollapseState]::Expanded
}
function Wait-SmokeExpanded($title, [bool]$expanded, $how) {
  Wait-UI "$title $(if ($expanded) { 'expanded' } else { 'collapsed' }) by $how" {
    $toggle = Find-UI $title $button
    if ($toggle -and (Get-SmokeExpanded $toggle) -eq $expanded) { $toggle }
  }
}
function Test-CaptureFallbackTable {
  $scope = Join-Path $root 'attribution-scope'
  $left = Join-Path $scope 'left'
  $right = Join-Path $scope 'right'
  [IO.Directory]::CreateDirectory($left) | Out-Null
  [IO.Directory]::CreateDirectory($right) | Out-Null
  [IO.File]::WriteAllBytes((Join-Path $left 'original.bin'), [byte[]]::new(1MB))
  [IO.File]::WriteAllBytes((Join-Path $left 'notes.bin'), [byte[]]::new(64KB))
  [IO.File]::WriteAllBytes((Join-Path $right 'second.bin'), [byte[]]::new(256KB))
  Set-Scope $scope
  Invoke-UI (Wait-Enabled 'Review scope' $true)
  Invoke-UI (Wait-Enabled 'Start analysis' $true)
  $null = Wait-UI 'Scan finished' { Find-UI 'Scan finished' $textType } 60
  Wait-Details 'Display options' 'Size basis'
  $basis = Wait-UI 'size basis selector' {
    Find-UI 'Size basis' ([Windows.Automation.ControlType]::ComboBox)
  }
  $basis.SetFocus()
  $null = Wait-UI 'keyboard focus on the size basis selector' {
    $focused = Find-UI 'Size basis' ([Windows.Automation.ControlType]::ComboBox)
    $focused -and $focused.Current.HasKeyboardFocus -and -not $focused.Current.IsOffscreen
  }
  # The closed native selector moves directly to its last option, unique physical size.
  Send-SmokeKey 0x23 # VK_END
  $null = Wait-UI 'unique physical size as the result metric' {
    Find-UI 'Unique physical size' ([Windows.Automation.ControlType]::Group)
  }
  Invoke-UI (Wait-Enabled 'Browse folders' $true)
  $null = Wait-UI 'left' { Find-UI 'left' $null }
  Invoke-UI (Wait-Enabled 'Show space distribution' $true)
  $heading = Get-UiName 'No item sizes are available'
  $null = Wait-UI 'unavailable notice' { Find-UI $heading $null } 60
  $title = Get-UiName 'Items in this folder'
  # The table opens expanded with the folder's rows.
  $toggle = Wait-SmokeExpanded $title $true 'opening'
  $null = Wait-UI 'fallback rows' { Find-UI 'right' $null }
  Save-SmokeCapture '01-fallback-table-expanded'
  if ($toggle.Current.IsOffscreen) { throw "$title is outside the window" }
  $bounds = $toggle.Current.BoundingRectangle
  $process.Refresh()
  $x = [int]($bounds.Left + $bounds.Width / 2)
  $y = [int]($bounds.Top + $bounds.Height / 2)
  [DiskUnfoldSmoke.Keyboard]::Click($process.MainWindowHandle, $x, $y)
  $toggle = Wait-SmokeExpanded $title $false 'mouse'
  Save-SmokeCapture '02-fallback-table-collapsed'
  [DiskUnfoldSmoke.Keyboard]::Click($process.MainWindowHandle, $x, $y)
  $toggle = Wait-SmokeExpanded $title $true 'mouse'
  $toggle.SetFocus()
  Send-SmokeKey 0x0D # VK_RETURN
  $toggle = Wait-SmokeExpanded $title $false 'Enter'
  Send-SmokeKey 0x20 # VK_SPACE
  $null = Wait-SmokeExpanded $title $true 'Space'
  "Fallback table: $heading; $title folds and unfolds by mouse, Enter and Space (aria-expanded); $Locale, $Theme, window $(if ($WindowSize) { $WindowSize } else { 'default' })"
}
function Test-CaptureTour {
  Set-Scope $scanRoot
  Invoke-UI (Wait-Enabled 'Review scope' $true)
  $null = Wait-Enabled 'Start analysis' $true
  Save-SmokeCapture '01-scope-reviewed'
  Invoke-UI (Wait-Enabled 'Start analysis' $true)
  $null = Wait-UI 'Scan finished' { Find-UI 'Scan finished' $textType }
  Save-SmokeCapture '02-analysis-result'
  Invoke-UI (Wait-Enabled 'Explain volume usage' $true)
  $null = Wait-UI 'volume explanation' { Find-UI 'Selected scanned folder' $textType }
  Save-SmokeCapture '03-volume-explanation'
  Invoke-UI (Wait-Enabled 'Hide volume explanation' $true)
  Invoke-UI (Wait-Enabled 'View folder details' $true)
  Wait-Details 'Space measurement details' 'Observed item space'
  $null = Wait-UI 'folder details' { Find-UI 'Observed item space' $null }
  Save-SmokeCapture '04-folder-details'
  Invoke-UI (Wait-Enabled 'Close details' $true)
  Invoke-UI (Wait-Enabled 'Browse folders' $true)
  $null = Wait-UI 'root.bin' { Find-UI 'root.bin' $null }
  Save-SmokeCapture '05-browse-folders'
  Invoke-UI (Wait-Enabled 'root.bin' $true)
  $null = Wait-UI 'file details' { Find-UI 'File details' $textType }
  Save-SmokeCapture '06-file-details'
  Invoke-UI (Wait-Enabled 'Close details' $true)
  "Capture tour: $Locale, $Theme, window $(if ($WindowSize) { $WindowSize } else { 'default' })"
}
# Best effort: a scope over the synchronous limit (200,000 files) classifies as a job, which
# shows "Classifying…". The job may finish before it is seen, so a miss is only a warning.
function Test-VirtualGroupsProgress {
  $large = Join-Path $root 'group-large'
  for ($folder = 0; $folder -lt 20; $folder++) {
    $path = Join-Path $large "d$folder"
    [IO.Directory]::CreateDirectory($path) | Out-Null
    for ($index = 0; $index -lt 10001; $index++) { [IO.File]::Create((Join-Path $path "f$index.dat")).Dispose() }
  }
  $entry = Open-VirtualGroups $large $null
  Invoke-UI $entry
  $null = Wait-UI 'virtual group screen' { Find-UI 'Group by my rules' $null }
  Invoke-UI (Wait-Enabled 'Start classifying' $true)
  try {
    $null = Wait-UI 'running classification' { Find-UI 'Classifying…' $null } 60
    $null = Save-SmokeCapture '07-classifying'
    'F36 native progress: "Classifying…" was shown for a job-sized scope'
    $cancel = Find-UI 'Cancel classification' $button
    if ($cancel -and $cancel.Current.IsEnabled) { Invoke-UI $cancel }
  } catch { Write-Warning "The running state was not seen (the job may have finished first): $($_.Exception.Message)" }
  $null = Wait-UI 'classification settled' { -not (Find-UI 'Classifying…' $null) } 120
  Invoke-UI (Wait-Enabled 'Back to folders' $true)
  Invoke-UI (Wait-Enabled 'Back to analysis' $true)
}
# F36: classify a real scan with two user rules; files stay unchanged and rules persist.
# F36 (until codex/f36-sample-rules ships): in the sample experience the virtual-group entry is
# shown disabled with its reason beside it, and pressing it opens nothing. Capture 08.
function Test-VirtualGroupsSample {
  Invoke-UI (Wait-Enabled 'Try sample data first' $true)
  $null = Wait-UI 'sample browse' { Find-UI 'Sample data' $null } 120
  $entry = Wait-UI 'sample virtual group entry' { Find-UI 'View as virtual groups' $button } 60
  $null = Wait-UI 'sample entry reason' { Find-UI 'Not available in the sample yet' $null }
  $null = Save-SmokeCapture '08-sample-entry-disabled'
  try { Invoke-UI $entry } catch { Write-Warning "Disabled entry press: $($_.Exception.Message)" }
  Start-Sleep -Milliseconds 800
  if (Find-UI 'Edit rules' $button) { throw 'The disabled sample entry opened the virtual group screen' }
  'F36 native sample: "View as virtual groups" stays disabled in the sample with "Not available in the sample yet" beside it'
  # End the experience (and confirm if asked) so the later steps start on the analysis screen.
  $null = Wait-UI 'sample ended' {
    $entry = Find-UI 'Try sample data first' $button
    if ($entry -and -not $entry.Current.IsOffscreen) { return $entry }
    $end = Find-UI 'End experience' $button
    if ($end -and $end.Current.IsEnabled) { try { Invoke-UI $end } catch { Write-Warning "End experience: $($_.Exception.Message)" } }
  } 60
}
# Captures follow the review list: 01 entry, 02 result with a selected row, 03 no saved rules,
# 04 editor with an empty value, 05 discard dialog, 06 members and reasons, 07 running (optional),
# 09 editor beside the applied result, 10 after restart. 08 (the sample's disabled entry) cannot be
# reached here: the 0.0.1 desktop UI has no way to open the sample.
function Test-VirtualGroups {
  $fixture = Join-Path $root 'group-scope'
  [IO.Directory]::CreateDirectory((Join-Path $fixture 'sub')) | Out-Null
  [IO.File]::WriteAllBytes((Join-Path $fixture 'a.mp4'), [byte[]]::new(10))
  [IO.File]::WriteAllBytes((Join-Path $fixture 'b.mp4'), [byte[]]::new(20))
  [IO.File]::WriteAllBytes((Join-Path $fixture 'c.txt'), [byte[]]::new(5))
  [IO.File]::WriteAllBytes((Join-Path $fixture 'sub\d.mp4'), [byte[]]::new(7))
  $before = (Get-FixtureState $fixture) -join [Environment]::NewLine
  # A failed focus check is reported after the remaining steps have run.
  $deferred = @()
  try {
    $entry = Open-VirtualGroups $fixture '42 bytes'
    $null = Save-SmokeCapture '01-browse-entry'
    Invoke-UI $entry
    $null = Wait-UI 'virtual group screen' { Find-UI 'Group by my rules' $null }
    'F36 native entry: "View as virtual groups" opened the screen'

    # No saved rules yet: "Edit rules" is the next step and "Back to folders" is drawn once.
    $null = Wait-UI 'no saved rules' { Find-UI 'No rules are saved yet.' $null }
    if ((Get-ButtonCount 'Back to folders') -ne 1) { throw "Expected one 'Back to folders' button, found $(Get-ButtonCount 'Back to folders')" }
    # With no saved rules there is nothing to classify: "Edit rules" is the only next step.
    if (Find-UI 'Start classifying' $button) { throw "'Start classifying' was offered although no rules are saved" }
    $null = Save-SmokeCapture '03-no-saved-rules'
    'F36 native no-rules screen: "No rules are saved yet.", one "Back to folders", no "Start classifying"'

    Invoke-UI (Wait-Enabled 'Edit rules' $true)
    $null = Wait-UI 'rule editor' { Find-UI 'Edit group rules' $null }
    Invoke-UI (Wait-Enabled 'Add group' $true)
    Set-GroupField 'Group name' 'Video'
    # The new group has one name condition with no value: the row is marked and Save says why it is off.
    $null = Wait-UI 'empty-value mark' { Find-UI 'Enter a value for the name condition.' $null }
    $null = Wait-UI 'empty-value reason' { Find-UI 'A condition has no value, so the rules cannot be saved.' $null }
    $null = Wait-Enabled 'Save' $false
    $null = Save-SmokeCapture '04-editor-empty-value'
    'F36 native editor: a condition with no value is marked and Save is off with its reason'
    Set-GroupField 'Enter a value' '.mp4'
    Select-GroupOption 'Comparison' 0x23 'Ends with' # VK_END: last option
    Invoke-UI (Wait-Enabled 'Add group' $true)
    Set-GroupField 'Group name' 'Big'
    Invoke-UI (Wait-Enabled 'Add size condition' $true)
    Invoke-UI (Wait-Enabled 'Remove condition 1' $true)
    Set-GroupField 'Enter a value' '10'
    # Leaving with edits asks first; keeping them returns to the editor.
    Invoke-UI (Wait-Enabled 'Cancel' $true)
    $null = Wait-UI 'discard dialog' { Find-UI 'Discard your changes?' $null }
    $null = Save-SmokeCapture '05-discard-dialog'
    Invoke-UI (Wait-Enabled 'Keep editing' $true)
    $null = Wait-UI 'editor kept' { -not (Find-UI 'Discard your changes?' $null) -and (Find-UI 'Edit group rules' $null) }
    Invoke-UI (Wait-Enabled 'Save' $true)
    $null = Wait-UI 'saved rules' {
      -not (Find-UI 'Edit group rules' $null) -and (Find-UI 'Video + Big' $null)
    }
    'F36 native rule editor: saved "Video" (name ends with .mp4) and "Big" (logical size at least 10)'

    Invoke-UI (Wait-Enabled 'Start classifying' $true)
    $table = Wait-UI 'virtual group summary' { Find-UI 'Virtual group summary' $null } 60
    $null = Wait-UI 'summary rows' { (Get-UINames $table) -contains 'Show members of Big' }
    $names = @(Get-UINames $table)
    $video = @(Get-RowCounts $names 'Show members of Video' @('Show members of Big', 'Unclassified'))
    $big = @(Get-RowCounts $names 'Show members of Big' @('Show members of Video', 'Unclassified'))
    if ($video[0] -ne '3 items' -or $big[0] -ne '2 items') {
      throw "Expected member counts 3 and 2, observed Video=$($video -join ',') Big=$($big -join ',')"
    }
    $categories = @(Get-UINames (Find-UI 'Categories' $null))
    $overlaps = Get-TabCount $categories 'Overlaps'
    $unclassified = Get-TabCount $categories 'Unclassified'
    if ($overlaps -ne 2 -or $unclassified -ne 1) {
      throw "Expected 2 overlaps and 1 unclassified in the categories, observed $overlaps and $($unclassified): $($categories -join ' | ')"
    }
    'F36 native summary: Video 3 items, Big 2 items; overlaps 2, unclassified 1 (sizes: {0})' -f (($names | Where-Object { $_ -match ' bytes$' }) -join ', ')
    Invoke-UI (Wait-Enabled 'Show members of Big' $true)
    $null = Wait-UI 'members of Big' { Find-UI 'Members: Big' $null }
    $null = Wait-UI 'member rows of Big' { Find-UI 'b.mp4' $button }
    $null = Save-SmokeCapture '02-result-row-selected'
    if ((Get-ButtonCount 'Back to folders') -ne 1 -or -not (Find-UI 'Member list' $button)) {
      throw "Expected one 'Back to folders' and the 'Member list' action on the result screen"
    }

    $totals = Wait-UI 'combined total' {
      $section = Find-UI 'Combined total' $null
      if ($section -and (Get-UINames $section) -contains 'Combined total, Logical size') { $section }
    }
    $totalNames = @(Get-UINames $totals)
    $label = [Array]::IndexOf($totalNames, 'Combined total, Logical size')
    $following = @($totalNames[($label + 1)..([Math]::Min($label + 3, $totalNames.Count - 1))])
    if ($following -notcontains '37 bytes') { throw "Combined logical total was not 37 bytes: $($totalNames -join ' / ')" }
    'F36 native combined total: 37 bytes, files in several groups counted once'

    # Members of the selected group with their paths, then the reasons of one member.
    Invoke-UI (Wait-Enabled 'Member list' $true)
    Invoke-UI (Wait-UI 'member a.mp4' { Find-UI 'a.mp4' $button })
    $null = Wait-UI 'classification reasons' { Find-UI 'Classification reasons' $null }
    $null = Save-SmokeCapture '06-members-and-reasons'
    Invoke-UI (Wait-Enabled 'Close reasons' $true)
    'F36 native members: the member list opened and one member showed its classification reasons'

    # While the editor is open, the applied result stays on its left and Back is off.
    Invoke-UI (Wait-Enabled 'Edit rules' $true)
    $null = Wait-UI 'editor beside the result' { (Find-UI 'Edit group rules' $null) -and (Find-UI 'Virtual group summary' $null) }
    $null = Wait-Enabled 'Back to folders' $false
    if ((Get-ButtonCount 'Back to folders') -ne 1) { throw "Expected one 'Back to folders' button while editing" }
    $null = Save-SmokeCapture '09-editing-with-result'
    Invoke-UI (Wait-Enabled 'Cancel' $true)
    $null = Wait-UI 'editor closed without changes' { -not (Find-UI 'Edit group rules' $null) }
    'F36 native editing: the applied result stayed visible beside the editor; Back was off'

    Invoke-UI (Wait-Enabled 'Back to folders' $true)
    try {
      $null = Wait-UI 'focus on the virtual group entry' {
        $focused = $ae::FocusedElement
        $focused -and $focused.Current.Name -ceq 'View as virtual groups' -and -not (Find-UI 'Group by my rules' $null)
      }
    } catch {
      $focused = $ae::FocusedElement
      $observed = if ($focused) { '{0} "{1}"' -f $focused.Current.ControlType.ProgrammaticName, $focused.Current.Name } else { 'none' }
      $deferred += "Focus did not return to 'View as virtual groups'; focused element: $observed"
      Write-Warning $deferred[-1]
    }
    if ($deferred.Count -eq 0) { 'F36 native return: focus is on "View as virtual groups"' }
    $after = (Get-FixtureState $fixture) -join [Environment]::NewLine
    if ($after -ne $before) {
      $changes = Compare-Object ($before -split [Environment]::NewLine) ($after -split [Environment]::NewLine) |
        ForEach-Object { '{0} {1}' -f $_.SideIndicator, $_.InputObject }
      throw "The fixture changed during classification: $($changes -join '; ')"
    }
    $files = @(Get-ChildItem -LiteralPath $fixture -Recurse -File | ForEach-Object { '{0}={1}' -f $_.FullName.Substring($fixture.Length + 1), $_.Length } | Sort-Object)
    if (($files -join ',') -ne 'a.mp4=10,b.mp4=20,c.txt=5,sub\d.mp4=7') { throw "Unexpected fixture files: $($files -join ',')" }
    'F36 fixture unchanged: ' + ($files -join ', ')

    Restart-SmokeApplication
    $entry = Open-VirtualGroups $fixture '42 bytes'
    Invoke-UI $entry
    $null = Wait-UI 'stored rules after restart' { Find-UI 'Video + Big' $null }
    $null = Save-SmokeCapture '10-after-restart'
    Invoke-UI (Wait-Enabled 'Back to folders' $true)
    'F36 native persistence: reopened on the same data folder and the saved rules "Video + Big" remained'
    if (((Get-FixtureState $fixture) -join [Environment]::NewLine) -ne $before) { throw 'The fixture changed after restart' }
    Invoke-UI (Wait-Enabled 'Back to analysis' $true)
    if ($VerifyVirtualGroupsProgress) { Test-VirtualGroupsProgress }
    if ($deferred.Count -gt 0) { throw ($deferred -join '; ') }
  } catch {
    $treePath = try { Write-UITree 'virtual-groups-failure' } catch { '<tree unavailable>' }
    Write-Warning "Virtual group UI tree: $treePath"
    throw
  }
}try {
  $window = Wait-UI 'main window' {
    $process.Refresh()
    if ($process.MainWindowHandle -ne 0) { $ae::FromHandle($process.MainWindowHandle) }
  }
  if ($WindowSize) { Set-SmokeWindowSize $WindowSize }
  # The smoke assertions use English UIA names. Normalize a Korean startup
  # profile for every run, including runs that do not exercise locale switching.
  $startupLanguage = Wait-UI 'initial locale' {
    if (Find-UI '언어 및 표시' $button) { 'ko' }
    elseif (Find-UI 'Language and display' $button) { 'en' }
  }
  $captureOnly = $CaptureTour -or $CaptureTreemap -or $CaptureFallbackTable -or $CaptureTreemapLink
  if ($startupLanguage -eq 'ko' -and -not $captureOnly) {
    Select-SmokeLanguage '언어 및 표시' '표시 언어' 'English' 'Display language' 'Close'
  }
  $null = Wait-UI 'path field' { Find-UI 'Windows folder path' $edit }
  if ($CaptureTour) { Test-CaptureTour }
  if ($CaptureTreemap) { Test-CaptureTreemap }
  if ($CaptureFallbackTable) { Test-CaptureFallbackTable }
  if ($CaptureTreemapLink) { Test-CaptureTreemapLink }
  if (-not $captureOnly) {
    $null = Wait-Enabled 'Start analysis' $false
    if ($VerifyInitialDrive) { Test-InitialDrive }
    if ($VerifyLocaleSwitch -or $VerifyKoreanIme) { Test-LocaleSwitch }
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
    if ($VerifyVirtualGroupsSample) { Test-VirtualGroupsSample }
    if ($VerifyVirtualGroups -or $VerifyVirtualGroupsProgress) { Test-VirtualGroups }
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
    $null = Wait-Enabled 'Start analysis' $false
    'F01 invalid folder: rejected, Start analysis disabled'
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
    $null = Wait-Enabled 'Start analysis' $true
    Set-Scope $child
    $null = Wait-Enabled 'Start analysis' $false
    Set-Scope $scanRoot
    $null = Wait-Enabled 'Start analysis' $false
    Invoke-UI (Wait-Enabled 'Review scope' $true)
    'F01 changing a reviewed path: new review required'
    Invoke-UI (Wait-Enabled 'Start analysis' $true)
    $null = Wait-UI 'Scan finished' { Find-UI 'Scan finished' $textType }
    $null = Wait-UI 'Result version: Final' { Find-ResultVersionLabel 'Final' }
    $null = Wait-UI 'allocated size as the default result metric' {
      Find-UI 'Allocated size' ([Windows.Automation.ControlType]::Group)
    }
    $null = Wait-UI '18-byte logical total' {
      Find-LogicalSize '18 bytes'
    }
    'F02/F03 native scan: Scan finished, Result version: Final, 18 bytes'
    if ($VerifyLocaleSwitch) {
      Select-SmokeLanguage 'Language and display' 'Display language' '한국어' '표시 언어' '닫기'
      $null = Wait-UI 'localized pinned result' {
        if (Find-UI '디스크 공간 분석 결과' $null) { Find-ResultVersionLabel '최종' '결과 판: ' }
      }
      Select-SmokeLanguage '언어 및 표시' '표시 언어' 'English' 'Display language' 'Close'
      $null = Wait-FinalLogicalSize '18 bytes'
      'F13 native locale: switched presentation on the same pinned result'
    }
    if ($VerifyRecentScope) { Test-RecentScopeReopen }
    if ($VerifyOperationHistory) { Test-OperationHistory }
    $null = Wait-VisibleAction 'Analyze again'
    $null = Wait-UI 'target editor collapsed after accepted scan' { -not (Find-UI 'Windows folder path' $edit) }
    Set-Scope $child
    Invoke-UI (Wait-VisibleAction 'Review scope')
    $null = Wait-Enabled 'Start analysis' $true
    Invoke-UI (Wait-VisibleAction 'Cancel selection')
    $null = Wait-UI 'discarded scope and retained final total' {
      $field = Find-UI 'Windows folder path' $edit
      if ($field -and (Find-ResultVersionLabel 'Final') -and
          $field.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).Current.Value -eq '') {
        Find-LogicalSize '18 bytes'
      }
    }
    $null = Wait-Enabled 'Start analysis' $false
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
  }
} catch {
  if ($VerifyVirtualGroups -or $VerifyVirtualGroupsProgress -or $VerifyVirtualGroupsSample -or $CaptureDirectory) {
    $treePath = try { Write-UITree 'failure' } catch { '<tree unavailable>' }
    Write-Warning "UI tree at failure: $treePath"
    # Keep the evidence next to the review captures when a capture folder was given.
    if ($CaptureDirectory -and (Test-Path -LiteralPath $treePath)) {
      [IO.Directory]::CreateDirectory($CaptureDirectory) | Out-Null
      Copy-Item -LiteralPath $treePath -Destination $CaptureDirectory -Force
      $null = Save-SmokeCapture 'zz-failure'
    }
  }
  Write-Warning "Native smoke failed. Isolated fixture and logs: $root"
  throw
} finally {
  if ($Theme -eq 'dark') {
    if ($null -ne $originalAppTheme) { Set-ItemProperty -LiteralPath $themeKey -Name AppsUseLightTheme -Type DWord -Value $originalAppTheme }
    else { Remove-ItemProperty -LiteralPath $themeKey -Name AppsUseLightTheme -ErrorAction SilentlyContinue }
  }
  if (-not $process.HasExited) {
    $null = $process.CloseMainWindow()
    if (-not $process.WaitForExit(10000)) { Stop-Process -Id $process.Id; throw 'The test process did not exit' }
  }
}
$restartLog = Join-Path $root 'stderr-restart.txt'
if ($process.ExitCode -ne 0 -or (Get-Item (Join-Path $root 'stderr.txt')).Length -ne 0 -or
    ((Test-Path -LiteralPath $restartLog) -and (Get-Item -LiteralPath $restartLog).Length -ne 0)) { throw "Abnormal exit, log at $root" }
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
      # 145 is ERROR_DIR_NOT_EMPTY and can occur while WebView2 finishes profile cleanup.
      if ($windowsError -notin @(32, 33, 145)) { throw }
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
if ($KeepFixture) { "Fixture kept: $root" } else { Remove-SmokeFixture -FixtureRoot $root }

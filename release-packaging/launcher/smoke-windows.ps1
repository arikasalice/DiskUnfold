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
  # dark makes the WebView report prefers-color-scheme: dark; the app follows it. contrast turns on
  # Windows high contrast for the run, so the WebView draws with forced-colors: active (#201).
  [ValidateSet('light', 'dark', 'contrast')][string]$Theme = 'light',
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
  # Capture the analysis, chart and table progress rows on a scope large enough to show them.
  [switch]$CaptureProgress,
  # Capture the treemap on the unique physical size basis, which NTFS can count (#149).
  [switch]$CaptureUniquePhysical,
  # Capture a histogram with an empty interval between two drawn ones, on both axes (F06 README 43).
  [switch]$CaptureHistogramGaps,
  # Capture the type legend: treemap and bar by type, a chosen class, patterns, None (F06 FR6).
  [switch]$CaptureTypeLegend,
  # Capture the type legend with a ninth class (a junction), folded and opened (F06 FR6).
  [switch]$CaptureTypeLegendMore,
  # Capture the treemap narrowed to one type from the legend, its table, a folder without the
  # type and the histogram with the chip inactive (#301); the Short form only the treemap.
  [switch]$CaptureTypeNarrowing,
  [switch]$CaptureTypeNarrowingShort,
  # Capture "Show in list": the files of the narrowed type below the folder, then sorted by
  # name (#306); the Short form only the opened list, for the dark and narrow runs.
  [switch]$CaptureTypeList,
  [switch]$CaptureTypeListShort,
  # Capture the bar chart of many items: 20 bars and a broken Other, then narrowed (#320).
  [switch]$CaptureTypeNarrowingMany,
  # Capture a bar chart whose Other is no larger than its largest bar: not broken (#320).
  [switch]$CaptureBarOtherSmall,
  # Capture the operation list of the operation history after one analysis (#170).
  [switch]$CaptureHistory,
  # Capture the Scan status column of the browse table with a child folder that cannot be listed (#133).
  [switch]$CaptureScanStatus,
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

    [DllImport("user32.dll")]
    private static extern bool SetForegroundWindow(IntPtr window);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern uint SendInput(uint count, Input[] inputs, int size);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool SetCursorPos(int x, int y);

    // Another window may take the foreground for a moment on a runner (a console, a
    // notification). Ask for it back until the window owns it or the time runs out;
    // returns how many requests it took (0 when the window already had it). Press and
    // Click still refuse to send when the window does not own the foreground.
    public static int Foreground(IntPtr window, int timeoutMs) {
      if (window == IntPtr.Zero) return 0;
      var requests = 0;
      var deadline = DateTime.UtcNow.AddMilliseconds(timeoutMs);
      while (GetForegroundWindow() != window && DateTime.UtcNow < deadline) {
        SetForegroundWindow(window);
        requests++;
        System.Threading.Thread.Sleep(100);
      }
      return requests;
    }

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
      # A message made only of placeholders, such as a bare "{count}", would match every name and
      # rewrite it; it names nothing the smoke looks up, so it is not a template.
      if (($source -replace '\{\w+\}', '').Trim() -eq '') { continue }
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
function Initialize-SmokeLocale([string]$language, [string]$scale = '100', [switch]$SeedHistory) {
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
  # History captures seed one old completed record using a different kind than the fresh scan.
  $seed = @'
import sqlite3, sys, uuid
connection = sqlite3.connect(sys.argv[1])
connection.execute("INSERT OR REPLACE INTO user_env_setting (setting_key, value_json, modified_source, created_at_ms, updated_at_ms, row_version) VALUES ('locale.display_language', ?, 'owner', 0, 0, 1)", ('"' + sys.argv[2] + '"',))
if sys.argv[3] != "100":
    connection.execute("INSERT OR REPLACE INTO user_env_setting (setting_key, value_json, modified_source, created_at_ms, updated_at_ms, row_version) VALUES ('a11y.app_scale', ?, 'owner', 0, 0, 1)", (sys.argv[3],))
connection.execute("UPDATE user_env_setting_version SET settings_version = settings_version + 1, updated_at_ms = 0 WHERE version_key = 'settings'")
if sys.argv[4] == "1":
    job_id = uuid.uuid4().bytes
    old_ms = 1577836800000
    record_seq = connection.execute("SELECT last_record_seq + 1 FROM job_runtime_history_state WHERE state_key = 'history'").fetchone()[0]
    connection.execute(
        """
        INSERT INTO job_runtime_job_start
            (job_id, kind_id, kind_version, scope_summary, started_at_ms,
             record_origin, created_at_ms)
        VALUES (?, 'duplicate.check', 1, 'Old history retention fixture',
                ?, 'this_app', ?)
        """,
        (job_id, old_ms, old_ms),
    )
    connection.execute(
        """
        INSERT INTO job_runtime_job_record
            (job_id, kind_id, kind_version, scope_summary, final_state,
             completed_count, overwritten_count, failed_count, skipped_count,
             unprocessed_count, started_at_ms, finished_at_ms, correlation_id,
             created_at_ms)
        VALUES (?, 'duplicate.check', 1, 'Old history retention fixture',
                'done', 0, 0, 0, 0, 0, ?, ?, ?, ?)
        """,
        (job_id, old_ms, old_ms, uuid.uuid4().bytes, old_ms),
    )
    connection.execute(
        """
        INSERT INTO job_runtime_job_link
            (job_id, record_origin, reckon_at_ms, record_seq, created_at_ms)
        VALUES (?, 'this_app', ?, ?, ?)
        """,
        (job_id, old_ms, record_seq, old_ms),
    )
    connection.execute(
        """
        UPDATE job_runtime_history_state
        SET last_record_seq = ?, row_version = row_version + 1, updated_at_ms = ?
        WHERE state_key = 'history'
        """,
        (record_seq, old_ms),
    )
connection.commit()
connection.close()
'@
  $seedHistoryValue = if ($SeedHistory) { '1' } else { '0' }
  $seed | & $python.Source - $database $language $scale $seedHistoryValue
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
$seedHistory = $CaptureHistory -or $VerifyOperationHistory
if ($Locale -ne 'en' -or $AppScale -ne '100' -or $seedHistory) {
  Initialize-SmokeLocale $Locale $AppScale -SeedHistory:$seedHistory
}
# The WebView reports prefers-color-scheme from the Windows app color mode, which the app follows. Set
# it for this run only and put it back in the final block below.
$themeKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize'
$originalAppTheme = (Get-ItemProperty -LiteralPath $themeKey -Name AppsUseLightTheme -ErrorAction SilentlyContinue).AppsUseLightTheme
if ($Theme -eq 'dark') {
  if (-not (Test-Path -LiteralPath $themeKey)) { $null = New-Item -Path $themeKey -Force }
  Set-ItemProperty -LiteralPath $themeKey -Name AppsUseLightTheme -Type DWord -Value 0
}
# High contrast is a session-wide setting; it is turned off again in the final block below.
if ($Theme -eq 'contrast') {
  Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class SmokeContrast {
  [StructLayout(LayoutKind.Sequential)]
  struct HighContrast { public int Size; public int Flags; public IntPtr Scheme; }
  [DllImport("user32.dll", SetLastError = true)]
  static extern bool SystemParametersInfo(int action, int parameter, ref HighContrast value, int update);
  static HighContrast Blank() { var value = new HighContrast(); value.Size = Marshal.SizeOf(typeof(HighContrast)); return value; }
  // SPI_GETHIGHCONTRAST; HCF_HIGHCONTRASTON is bit 0.
  public static bool IsOn() { var value = Blank(); SystemParametersInfo(0x42, value.Size, ref value, 0); return (value.Flags & 1) != 0; }
  // SPI_SETHIGHCONTRAST with SPIF_UPDATEINIFILE | SPIF_SENDCHANGE, the last used scheme.
  public static bool Set(bool on) { var value = Blank(); value.Flags = on ? 1 : 0; return SystemParametersInfo(0x43, value.Size, ref value, 3); }
}
'@
  $originalContrast = [SmokeContrast]::IsOn()
  if (-not [SmokeContrast]::Set($true) -or -not [SmokeContrast]::IsOn()) { throw 'Windows high contrast could not be turned on' }
  Start-Sleep -Seconds 3
  'Windows high contrast is on for this run'
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
function Wait-TasksEntry {
  $taskName = Get-UiName 'Tasks'
  Wait-UI 'Tasks header action in view' {
    $window.FindAll($scopeTree, [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, $button)) |
      Where-Object {
        $name = $_.Current.Name
        $_.Current.IsEnabled -and -not $_.Current.IsOffscreen -and
          ($name -eq $taskName -or $name.StartsWith($taskName + ', '))
      } |
      Select-Object -First 1
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
# Brings the test window back to the foreground before input, for at most three seconds,
# and logs when it had to. A window that does not get it back still fails the input step.
function Request-SmokeForeground {
  $process.Refresh()
  $requests = [DiskUnfoldSmoke.Keyboard]::Foreground($process.MainWindowHandle, 3000)
  if ($requests -gt 0) { Write-Host "Foreground requested $requests time(s) before input" }
}
# Send a key only while the test window owns the foreground.
function Send-SmokeKey($key) {
  Request-SmokeForeground
  [DiskUnfoldSmoke.Keyboard]::Press($process.MainWindowHandle, $key)
}
# Click a screen point only while the test window owns the foreground.
function Send-SmokeClick($x, $y) {
  Request-SmokeForeground
  [DiskUnfoldSmoke.Keyboard]::Click($process.MainWindowHandle, $x, $y)
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
    $field = Find-UI 'Folder path' $edit
    if ($field) { return $field }
    $opener = Find-UI 'Analyze again' $button
    if ($opener -and $opener.Current.IsEnabled) { return $opener }
  }
  if ($entry.Current.ControlType -eq $button) {
    Expand-UI $entry
    $null = Wait-UI 'reopened target input' { Find-UI 'Folder path' $edit }
  }
}
function Set-Scope($path, $fieldName = 'Folder path') {
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
  Write-Host "Within 120 seconds, drag this isolated folder from Explorer onto the Folder path field: $child"
  $deadline = [DateTime]::UtcNow.AddSeconds(120)
  $received = $false
  do {
    if ($process.HasExited) { throw 'Application exited while waiting for native folder drop' }
    try {
      $field = Find-UI 'Folder path' $edit
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
      $field = Find-UI 'Folder path' $edit
      if ($field -and -not $field.Current.IsOffscreen) { $field }
    }
    Wait-ScopeActionsVisible
    $option.GetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern).Select()
    $null = Wait-UI 'drive path selected' {
      $field = Find-UI 'Folder path' $edit
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
  Expand-UI (Wait-TasksEntry)
  Invoke-UI (Wait-VisibleAction 'View all task history')
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
    $inputField = Find-UI 'Folder path' $edit
    if (-not $inputField) { $inputField = Find-UI '폴더 경로' $edit }
    $inputField
  }
  $value = $field.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern)
  $draftPath = Join-Path $root 'locale-scope'
  New-Item -ItemType Directory -Path $draftPath | Out-Null
  $value.SetValue($draftPath)
  if ($field.Current.Name -eq 'Folder path') {
    Select-SmokeLanguage 'Language and display' 'Display language' '한국어' '표시 언어' '닫기'
  } else {
    Select-SmokeLanguage '언어 및 표시' '표시 언어' 'English' 'Display language' 'Close'
    Select-SmokeLanguage 'Language and display' 'Display language' '한국어' '표시 언어' '닫기'
  }
  $localized = Wait-UI 'Korean path field' { Find-UI '폴더 경로' $edit }
  if ($localized.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).Current.Value -ne $draftPath) {
    throw 'Locale switch discarded the current scope draft'
  }
  if ($VerifyKoreanIme) { Test-KoreanIme $localized $draftPath }
  Select-SmokeLanguage '언어 및 표시' '표시 언어' 'English' 'Display language' 'Close'
  $english = Wait-UI 'English path field' { Find-UI 'Folder path' $edit }
  $english.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).SetValue('')
  'F13 native locale: switched between Korean and English without discarding the scope draft'
}
# Save the window as a PNG under %TEMP% when the interactive desktop allows it.
# True when the element is on screen and lies inside the window's bounds.
function Test-InWindow($item) {
  if ($item.Current.IsOffscreen) { return $false }
  $box = $item.Current.BoundingRectangle
  $frame = $window.Current.BoundingRectangle
  $box.Width -gt 0 -and $box.Top -ge $frame.Top -and $box.Bottom -le $frame.Bottom
}
# Brings an element into the window: its own scroll item pattern first, then the scrollable
# ancestors one page at a time. Returns whether the element ends up inside the window.
function Show-InWindow($item) {
  if (Test-InWindow $item) { return $true }
  $itemPattern = $null
  if ($item.TryGetCurrentPattern([Windows.Automation.ScrollItemPattern]::Pattern, [ref]$itemPattern)) {
    try { $itemPattern.ScrollIntoView(); Start-Sleep -Milliseconds 300 } catch {}
    if (Test-InWindow $item) { return $true }
  }
  $walker = [Windows.Automation.TreeWalker]::ControlViewWalker
  for ($parent = $walker.GetParent($item); $parent -and -not [Windows.Automation.Automation]::Compare($parent, $window); $parent = $walker.GetParent($parent)) {
    $scroll = $null
    if (-not $parent.TryGetCurrentPattern([Windows.Automation.ScrollPattern]::Pattern, [ref]$scroll)) { continue }
    if (-not $scroll.Current.VerticallyScrollable) { continue }
    for ($page = 0; $page -lt 30 -and -not (Test-InWindow $item); $page++) {
      $before = $scroll.Current.VerticalScrollPercent
      $scroll.Scroll([Windows.Automation.ScrollAmount]::NoAmount, [Windows.Automation.ScrollAmount]::LargeIncrement)
      Start-Sleep -Milliseconds 150
      if ($scroll.Current.VerticalScrollPercent -eq $before) { break }
    }
    if (Test-InWindow $item) { return $true }
  }
  try { $item.SetFocus(); Start-Sleep -Milliseconds 300 } catch {}
  Test-InWindow $item
}
# #146: after the volume explanation opens, scroll its last readout into the window so the capture
# shows the opened content. The lowest readout that exists is used, since what the explanation
# lists depends on the scan; the first one stands in when the read failed.
function Show-VolumeExplanation {
  foreach ($label in @('Volume difference', 'Volume usage summary', 'Selected scanned folder')) {
    $item = Find-UI $label $null
    if (-not $item) { continue }
    if (Show-InWindow $item) { return "Volume explanation in view: $(Get-UiName $label)" }
    Write-Warning "Volume explanation: '$(Get-UiName $label)' stays outside the window"
    return
  }
  Write-Warning 'Volume explanation: no readout found to scroll to'
}
function Save-SmokeCapture($name, [switch]$Now, [scriptblock]$Prepare) {
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
    # A progress capture skips this: its bar may be gone by then.
    if (-not $Now) { Start-Sleep -Milliseconds 700 }
    # A preparation that depends on the final layout (a scroll) runs after the resize above.
    if ($Prepare) { & $Prepare; Start-Sleep -Milliseconds 400 }
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
  $null = Wait-UI 'path field after restart' { Find-UI 'Folder path' $edit }
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
    ($names | Where-Object { $_ -like 'Tiny, *' })
  } 60
  if (Get-UINames | Where-Object { $_ -like 'tiny-*' }) { throw 'Tiny was expanded below its header band minimum' }
  Save-SmokeCapture '01-treemap-depth2'
  "Treemap depth 2: Photos holds album-a.bin and $group, Tiny stays one shape; $Locale, $Theme, window $(if ($WindowSize) { $WindowSize } else { 'default' })"
  # F06 #180 (README 34): the shapes are the options of one list. The screen opens on its title;
  # one Tab reaches the chart, arrows move on screen, Space selects, Enter on an item says why it
  # does nothing. Each step logs where the focus is and is captured.
  $focusName = {
    $focused = $ae::FocusedElement
    if ($focused) { '{0} "{1}"' -f $focused.Current.ControlType.ProgrammaticName, $focused.Current.Name } else { 'none' }
  }
  "Focus on entry: $(& $focusName)"
  Save-SmokeCapture '02-entry'
  Send-SmokeKey 0x09 # VK_TAB
  Start-Sleep -Milliseconds 300
  "Focus after one Tab: $(& $focusName)"
  Save-SmokeCapture '03-first-shape'
  # README 35: a folder stands where its header band is; Down reaches the first row inside it.
  Send-SmokeKey 0x28 # VK_DOWN
  Start-Sleep -Milliseconds 300
  "Focus after Down from the Videos header: $(& $focusName)"
  Save-SmokeCapture '03a-header-down'
  Send-SmokeKey 0x26 # VK_UP
  Start-Sleep -Milliseconds 300
  "Focus after Up from clip.bin: $(& $focusName)"
  Save-SmokeCapture '03b-child-up'
  Send-SmokeKey 0x27 # VK_RIGHT
  Start-Sleep -Milliseconds 300
  "Focus after Right: $(& $focusName)"
  Save-SmokeCapture '04-arrow'
  Send-SmokeKey 0x20 # VK_SPACE
  Start-Sleep -Milliseconds 300
  "Focus after Space: $(& $focusName)"
  Save-SmokeCapture '05-selected-focused'
  Send-SmokeKey 0x25 # VK_LEFT
  Start-Sleep -Milliseconds 300
  "Focus after Left: $(& $focusName)"
  Save-SmokeCapture '06-selected'
  $options = @($window.FindAll($scopeTree, [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, [Windows.Automation.ControlType]::ListItem)) |
    Where-Object { $_.Current.Name -like '*,*' })
  foreach ($option in @($options | Select-Object -First 6) + @($options | Where-Object { $_.Current.Name -like 'notes.bin*' })) {
    $item = $null
    $selected = if ($option.TryGetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern, [ref]$item)) { $item.Current.IsSelected } else { 'no pattern' }
    'Shape: {0} "{1}" selected={2}' -f $option.Current.ControlType.ProgrammaticName, $option.Current.Name, $selected
  }
  # Enter on a shape that is not a group: the status says why nothing opens.
  $file = $options | Where-Object { $_.Current.Name -like 'album-a.bin, *' } | Select-Object -First 1
  if ($file) {
    $file.SetFocus()
    Start-Sleep -Milliseconds 200
    Send-SmokeKey 0x0D # VK_RETURN
    Start-Sleep -Milliseconds 400
    $said = @($window.FindAll($scopeTree, [Windows.Automation.PropertyCondition]::new($ae::NameProperty, (Get-UiName "This item can't be opened or expanded here yet.")))).Count
    "Enter on album-a.bin: status says it cannot be opened or expanded here: $($said -gt 0)"
    Save-SmokeCapture '07-enter-item'
  } else { 'album-a.bin is not among the shapes' }
  # Enter on the group shape opens its list; closing returns the focus to that shape (README 22).
  $shape = Wait-UI "$group shape" {
    $options | Where-Object { $_.Current.Name -like "$group, *Photos*" } | Select-Object -First 1
  }
  $shape.SetFocus()
  Start-Sleep -Milliseconds 200
  Send-SmokeKey 0x0D # VK_RETURN
  $back = Wait-Enabled 'Back to chart' $true
  $back.SetFocus()
  Invoke-UI $back
  Start-Sleep -Milliseconds 500
  "Focus after closing the combined items: $(& $focusName)"
  Save-SmokeCapture '08-group-closed'
  # #227: the format strip switches the chart. A format may carry a "No data" mark after its
  # name, so the radio is found by the start of its name.
  $radioType = [Windows.Automation.ControlType]::RadioButton
  foreach ($step in @(@('Bar chart', '09-format-bar'), @('Histogram', '10-format-histogram'))) {
    $wanted = Get-UiName $step[0]
    $radio = Wait-UI "$($step[0]) format" {
      $window.FindAll($scopeTree, [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, $radioType)) |
        Where-Object { $_.Current.Name -like "$wanted*" } | Select-Object -First 1
    }
    $pattern = $null
    $radio.SetFocus()
    if ($radio.TryGetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern, [ref]$pattern)) { $pattern.Select() }
    else { Invoke-UI $radio }
    Start-Sleep -Milliseconds 2000
    if (Find-UI 'The chart could not be loaded.' $null) { throw "The $($step[0]) request was refused" }
    "Format $($step[0]): selected as '$($radio.Current.Name)'"
    Save-SmokeCapture $step[1]
  }
  # F06 README 43: the histogram opens on the item count; its vertical axis switches to size.
  # "Size" names more than one English text, so the size radio is taken as the group's second.
  $axisName = Get-UiName 'Vertical axis'
  $axis = Wait-UI 'Vertical axis' {
    $window.FindFirst($scopeTree, [Windows.Automation.PropertyCondition]::new($ae::NameProperty, $axisName))
  }
  $axisRadios = @($axis.FindAll($scopeTree, [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, $radioType)))
  if ($axisRadios.Count -ne 2) { throw "The vertical axis has $($axisRadios.Count) options, not two" }
  "Histogram axis on entry: $(@($axisRadios | ForEach-Object { $_.Current.Name }) -join ' | ')"
  $sizeAxis = $axisRadios[1]
  $pattern = $null
  $sizeAxis.SetFocus()
  if ($sizeAxis.TryGetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern, [ref]$pattern)) { $pattern.Select() }
  else { Invoke-UI $sizeAxis }
  Start-Sleep -Milliseconds 2000
  if (Find-UI 'The chart could not be loaded.' $null) { throw 'The size axis request was refused' }
  "Histogram axis: '$($sizeAxis.Current.Name)' selected"
  Save-SmokeCapture '11-histogram-size'
}
# README 31 review captures: a progress row is short-lived, so it is captured the moment it shows.
# $name is an English progress label, or '*' for the first progress bar of any name. Not seeing it in
# time is logged, not a failure: a fast runner can finish before the first look.
function Save-ProgressCapture([string]$name, [string]$capture, [int]$seconds = 4) {
  $condition = [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, [Windows.Automation.ControlType]::ProgressBar)
  $wanted = if ($name -eq '*') { $null } else { Get-UiName $name }
  $deadline = [DateTime]::UtcNow.AddSeconds($seconds)
  while ([DateTime]::UtcNow -lt $deadline) {
    $bar = @($window.FindAll($scopeTree, $condition) | Where-Object { -not $wanted -or $_.Current.Name -eq $wanted }) | Select-Object -First 1
    if ($bar) {
      $shown = $bar.Current.Name
      Save-SmokeCapture $capture -Now
      "Progress capture $($capture): $shown"
      return
    }
    Start-Sleep -Milliseconds 30
  }
  "Progress capture $($capture): not seen within $seconds s"
}
# F06 #164: progress rows for design review. A runner answers the small fixtures before a capture,
# so this scope holds 40,000 empty files: the analysis, the chart and its table take long enough to be
# seen. Each capture is taken the moment its progress bar shows; one not seen in time is logged.
function Test-CaptureProgress {
  $scope = Join-Path $root 'progress-scope'
  $many = Join-Path $scope 'many'
  [IO.Directory]::CreateDirectory($many) | Out-Null
  for ($index = 0; $index -lt 40000; $index++) {
    [IO.File]::Create((Join-Path $many "f$index.bin")).Dispose()
  }
  [IO.File]::WriteAllBytes((Join-Path $scope 'large.bin'), [byte[]]::new(8MB))
  Set-Scope $scope
  Invoke-UI (Wait-Enabled 'Review scope' $true)
  Invoke-UI (Wait-Enabled 'Start analysis' $true)
  Save-ProgressCapture '*' '01-analysis-progress' 20
  $null = Wait-UI 'Scan finished' { Find-UI 'Scan finished' $textType } 120
  Invoke-UI (Wait-Enabled 'Browse folders' $true)
  $null = Wait-UI 'many' { Find-UI 'many' $null } 60
  Invoke-UI (Wait-Enabled 'Show space distribution' $true)
  Save-ProgressCapture 'Loading chart' '02-chart-loading' 10
  Save-ProgressCapture 'Loading items in this folder' '03-folder-rows-loading' 10
  "Progress captures: $Locale, $Theme, window $(if ($WindowSize) { $WindowSize } else { 'default' }), app scale $AppScale%"
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
    # #175: the folder items table fits the window without scrolling sideways.
    # The scroll area's name fills a template with the table's own (translated) name.
    $tableName = Get-UiName 'Items in this folder'
    $areaName = (Get-UiName 'Items in this folder scroll area').Replace('Items in this folder', $tableName)
    $tableArea = Wait-UI 'folder items table' { Find-UI $areaName $null } 30
    $scroll = $null
    if (-not $tableArea.TryGetCurrentPattern([Windows.Automation.ScrollPattern]::Pattern, [ref]$scroll)) {
      'Folder items table: no scroll pattern, so it does not scroll'
    } elseif ($scroll.Current.HorizontallyScrollable) {
      throw "The folder items table scrolls sideways at app scale $AppScale%, window $WindowSize"
    } else { 'Folder items table: fits without scrolling sideways' }
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
    # chart's first shape brings the chart into the window; the head then lies above.
    $first = Wait-UI 'media.bin shape' {
      @($window.FindAll($scopeTree, [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, [Windows.Automation.ControlType]::ListItem)) |
        Where-Object { $_.Current.Name -like 'media.bin*' }) | Select-Object -First 1
    }
    $first.SetFocus()
    Start-Sleep -Milliseconds 800
    if ([int]$AppScale -ge 150) {
      $head = Find-UI 'Language and display' $button
      "Page scrolled to the chart buttons; the head is $(if ($head -and $head.Current.IsOffscreen) { 'above the window' } else { 'still in view' })"
    }
    Save-SmokeCapture '02-treemap-link-scrolled'
    # #167: the Tab order from the first shape, one capture per stop.
    for ($stop = 1; $stop -le 5; $stop++) {
      Send-SmokeKey 0x09 # VK_TAB
      Start-Sleep -Milliseconds 300
      $focused = $ae::FocusedElement
      $observed = if ($focused) { '{0} "{1}"' -f $focused.Current.ControlType.ProgrammaticName, $focused.Current.Name } else { 'none' }
      "Tab $stop from the first shape: $observed"
      Save-SmokeCapture "03-tab-$stop"
    }
    "Treemap with a junction: Docs is drawn with report.bin inside it, the junction has no area; $Locale, $Theme, window $(if ($WindowSize) { $WindowSize } else { 'default' })"
  } finally {
    [IO.Directory]::Delete((Join-Path $docs 'redirect'))
  }
}
# F06 #129 item 6: an unavailable chart opens the folder's table under its notice (#109), and the
# table title must fold and unfold with the mouse, Enter and Space and say so (aria-expanded).
# Every unavailable reason uses the same table. A runner cannot produce no_attribution: folders
# that share a hard link are counted as items not drawn. Since #149 NTFS counts the unique
# physical size, so the scope holds only folders that cannot be listed: none of them has an
# allocated or unique physical size, and the finished chart is unavailable with no_item_sizes.
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
# Chooses the unique physical size basis in the display options of a finished scan.
function Select-UniquePhysicalBasis {
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
  $user = [Security.Principal.WindowsIdentity]::GetCurrent().User
  $originals = @{}
  foreach ($folder in @($left, $right)) {
    $originals[$folder] = Get-Acl -LiteralPath $folder
    $acl = Get-Acl -LiteralPath $folder
    $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($user, 'ListDirectory', 'Deny'))
    Set-Acl -LiteralPath $folder -AclObject $acl
  }
  try { Show-FallbackTable $scope } finally {
    foreach ($folder in $originals.Keys) { Set-Acl -LiteralPath $folder -AclObject $originals[$folder] }
  }
}
function Show-FallbackTable($scope) {
  Set-Scope $scope
  Invoke-UI (Wait-Enabled 'Review scope' $true)
  Invoke-UI (Wait-Enabled 'Start analysis' $true)
  $null = Wait-UI 'Scan finished' { Find-UI 'Scan finished' $textType } 60
  Select-UniquePhysicalBasis
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
  $x = [int]($bounds.Left + $bounds.Width / 2)
  $y = [int]($bounds.Top + $bounds.Height / 2)
  Send-SmokeClick $x $y
  $toggle = Wait-SmokeExpanded $title $false 'mouse'
  Save-SmokeCapture '02-fallback-table-collapsed'
  Send-SmokeClick $x $y
  $toggle = Wait-SmokeExpanded $title $true 'mouse'
  $toggle.SetFocus()
  Send-SmokeKey 0x0D # VK_RETURN
  $toggle = Wait-SmokeExpanded $title $false 'Enter'
  Send-SmokeKey 0x20 # VK_SPACE
  $null = Wait-SmokeExpanded $title $true 'Space'
  "Fallback table: $heading; $title folds and unfolds by mouse, Enter and Space (aria-expanded); $Locale, $Theme, window $(if ($WindowSize) { $WindowSize } else { 'default' })"
}
# #149: NTFS without block cloning shares no extents between files, so the unique physical size
# basis counts every file and the chart draws both folders instead of an unavailable notice.
function Test-CaptureUniquePhysical {
  $scope = Join-Path $root 'unique-scope'
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
  Select-UniquePhysicalBasis
  Invoke-UI (Wait-Enabled 'Browse folders' $true)
  $null = Wait-UI 'left' { Find-UI 'left' $null }
  Invoke-UI (Wait-Enabled 'Show space distribution' $true)
  $heading = Get-UiName 'No item sizes are available'
  $null = Wait-UI 'left and right drawn on the unique physical size basis' {
    if (Find-UI $heading $null) { throw 'The unique physical size basis has no item sizes on NTFS' }
    $names = Get-UINames
    ($names | Where-Object { $_ -like 'left, *' }) -and ($names | Where-Object { $_ -like 'right, *' })
  } 60
  Save-SmokeCapture '01-unique-physical-treemap'
  "Unique physical treemap: left and right drawn; $Locale, $Theme, window $(if ($WindowSize) { $WindowSize } else { 'default' })"
}
# F06 README 43: a histogram keeps the place, name and zero value of an empty interval between
# drawn ones. Small files fill 1 KiB–1 MiB and one 1 GiB file fills 1 GiB–1 TiB, so 1 MiB–1 GiB is
# empty on the item count and on the size axis. SetLength allocates the large file without writing it.
# F06 FR6 (README 44): the type legend. One file of each type, an installer image, a name without
# an extension, an unknown extension and a small folder that stays one shape. Captures the treemap
# and the bar chart by type, a class chosen in the legend, patterns on, and the None criterion.
function Select-SmokeRadio($radio) {
  $pattern = $null
  $radio.SetFocus()
  if ($radio.TryGetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern, [ref]$pattern)) { $pattern.Select() }
  else { Invoke-UI $radio }
  Start-Sleep -Milliseconds 1500
}
# The type chip is named by its words, then "Clear type filter" (F06 README 46.2).
function Find-TypeChip {
  $suffix = ', ' + (Get-UiName 'Clear type filter')
  @($window.FindAll($scopeTree, [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, $button)) |
    Where-Object { $_.Current.Name -like "*$suffix" }) | Select-Object -First 1
}
# F06 #301 (README 46): narrowing the treemap to one type from the legend. Videos sit at the top
# and in a subfolder, so the legend's Videos row and the narrowed total count both (46.9). Captures
# the narrowed treemap, its table, a folder without videos, and the histogram with the chip
# inactive. With -Short only the narrowed treemap, for the dark and narrow runs.
function Test-CaptureTypeNarrowing([switch]$Short, [switch]$List) {
  $scope = Join-Path $root 'type-narrowing-scope'
  foreach ($folder in 'docs', 'songs') { [IO.Directory]::CreateDirectory((Join-Path $scope $folder)) | Out-Null }
  $files = [ordered]@{ 'clip.mp4' = 6MB; 'notes.txt' = 1MB; 'docs\report.pdf' = 10MB; 'docs\talk.mp4' = 4MB; 'songs\song.mp3' = 2MB }
  foreach ($name in $files.Keys) { [IO.File]::WriteAllBytes((Join-Path $scope $name), [byte[]]::new($files[$name])) }
  Set-Scope $scope
  Invoke-UI (Wait-Enabled 'Review scope' $true)
  Invoke-UI (Wait-Enabled 'Start analysis' $true)
  $null = Wait-UI 'Scan finished' { Find-UI 'Scan finished' $textType } 60
  Invoke-UI (Wait-Enabled 'Browse folders' $true)
  $null = Wait-UI 'clip.mp4' { Find-UI 'clip.mp4' $null }
  Invoke-UI (Wait-Enabled 'Show space distribution' $true)
  $legendName = Get-UiName 'Legend'
  $listType = [Windows.Automation.ControlType]::List
  $listItem = [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, [Windows.Automation.ControlType]::ListItem)
  $legend = Wait-UI 'type legend' {
    @($window.FindAll($scopeTree, [Windows.Automation.PropertyCondition]::new($ae::NameProperty, $legendName)) |
      Where-Object { $_.Current.ControlType -eq $listType }) | Select-Object -First 1
  } 60
  $videosName = Get-UiName 'Videos'
  $videos = Wait-UI 'Videos in the legend' { @($legend.FindAll($scopeTree, $listItem) | Where-Object { $_.Current.Name -like "$videosName*" }) | Select-Object -First 1 }
  "Legend Videos before narrowing: '$($videos.Current.Name)'"
  Select-SmokeRadio $videos
  $clear = Wait-UI 'the type chip' { Find-TypeChip }
  $textCondition = [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, $textType)
  $total = Wait-UI 'the narrowed total' {
    @($window.FindAll($scopeTree, $textCondition) | Where-Object { $_.Current.Name -like "$videosName *·*" }) | Select-Object -First 1
  }
  "Chip: '$($clear.Current.Name)'; status line: '$($total.Current.Name)'"
  if ($List) { Save-TypeListCaptures -Short:$Short; return }
  # Choosing a legend row can scroll a narrow window down to the legend; show the status line.
  Save-SmokeCapture '01-narrowed-treemap' -Prepare { $null = Show-InWindow $clear }
  if ($Short) { return }
  # The table under the chart lists the same shapes: docs and clip.mp4 by their video bytes (46.10).
  $docs = @($window.FindAll($scopeTree, [Windows.Automation.PropertyCondition]::new($ae::NameProperty, 'docs')))
  Save-SmokeCapture '02-narrowed-table' -Prepare { if ($docs.Count -gt 0) { $null = Show-InWindow $docs[-1] } }
  # The bar chart narrows too: clip.mp4 and docs by their video bytes (#301).
  $radioCondition = [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, [Windows.Automation.ControlType]::RadioButton)
  $barName = Get-UiName 'Bar chart'
  Select-SmokeRadio (Wait-UI 'Bar chart format' { @($window.FindAll($scopeTree, $radioCondition) | Where-Object { $_.Current.Name -like "$barName*" }) | Select-Object -First 1 })
  $barTotal = Wait-UI 'the narrowed bar total' { @($window.FindAll($scopeTree, $textCondition) | Where-Object { $_.Current.Name -like "$videosName *·*" }) | Select-Object -First 1 }
  "Bar status line: '$($barTotal.Current.Name)'"
  Save-SmokeCapture '02b-narrowed-bar' -Prepare { $null = Show-InWindow $clear }
  # The histogram does not narrow: the chip stays, inactive, with its note (46.5).
  $histogramName = Get-UiName 'Histogram'
  Select-SmokeRadio (Wait-UI 'Histogram format' { @($window.FindAll($scopeTree, $radioCondition) | Where-Object { $_.Current.Name -like "$histogramName*" }) | Select-Object -First 1 })
  $null = Wait-UI 'the inactive chip note' { Find-UI "Doesn't apply to this format" $null }
  Save-SmokeCapture '03-histogram-inactive'
  $treemapName = Get-UiName 'Treemap'
  Select-SmokeRadio (Wait-UI 'Treemap format' { @($window.FindAll($scopeTree, $radioCondition) | Where-Object { $_.Current.Name -like "$treemapName*" }) | Select-Object -First 1 })
  $null = Wait-UI 'the narrowed total again' { @($window.FindAll($scopeTree, $textCondition) | Where-Object { $_.Current.Name -like "$videosName *·*" }) | Select-Object -First 1 }
  # songs holds no video: the chart's place says so and offers to clear (46.1). The choice is
  # kept while the folders are shown, so open songs there and show its space distribution.
  Invoke-UI (Wait-Enabled 'Back to folders' $true)
  $songs = Wait-UI 'the songs folder row' { Find-UI 'songs' $button }
  Invoke-UI $songs
  $null = Wait-UI 'song.mp3' { Find-UI 'song.mp3' $null }
  Invoke-UI (Wait-Enabled 'Show space distribution' $true)
  $null = Wait-UI 'no videos in songs' {
    @($window.FindAll($scopeTree, $textCondition) | Where-Object { $_.Current.Name -like "*$videosName*" -and $_.Current.Name -notlike '*·*' -and $_.Current.Name -ne $videosName }) | Select-Object -First 1
  } 30
  Save-SmokeCapture '04-folder-without-type'
  "Type narrowing capture: $Locale, $Theme, window $(if ($WindowSize) { $WindowSize } else { 'default' })"
}
# F06 #306 (README 46.13): "Show in list" puts the files of the type below the chart's folder in
# the chart's place: clip.mp4 directly in the scope and talk.mp4 in docs, largest first. Locations
# start at the scope folder's name, so the docs location is the list's own text; below 720 px it
# sits under the name in the row header, whose name then holds both.
function Save-TypeListCaptures([switch]$Short) {
  Invoke-UI (Wait-UI 'Show in list' { Find-UI 'Show in list' $button })
  $null = Wait-UI 'the docs location in the list' {
    @($window.FindAll($scopeTree, [Windows.Automation.Condition]::TrueCondition) |
      Where-Object { $_.Current.Name -like '*type-narrowing-scope\docs*' }) | Select-Object -First 1
  } 60
  Save-SmokeCapture '05-file-list'
  if ($Short) { return }
  Invoke-UI (Wait-UI 'the Name header' { Find-UI 'Name' $button })
  Start-Sleep -Milliseconds 1500
  Save-SmokeCapture '06-file-list-by-name'
  Invoke-UI (Wait-Enabled 'Back to chart' $true)
  $null = Wait-UI 'Show in list after going back' { Find-UI 'Show in list' $button }
  "Type list capture: $Locale, $Theme, window $(if ($WindowSize) { $WindowSize } else { 'default' })"
}
# F06 #320 (README 47): a bar chart of 2,100 children draws the largest 20 and one Other, which
# is larger than any single bar and so drawn to the end with a break. Narrowed to videos, the
# Other of the remaining videos takes their colour with a dashed border.
function Test-CaptureTypeNarrowingMany {
  $scope = Join-Path $root 'type-narrowing-many-scope'
  [IO.Directory]::CreateDirectory($scope) | Out-Null
  for ($index = 0; $index -lt 2100; $index++) {
    $name = if ($index -lt 100) { "clip-$index.mp4" } else { "note-$index.txt" }
    [IO.File]::WriteAllBytes((Join-Path $scope $name), [byte[]]::new(4096 + $index))
  }
  Set-Scope $scope
  Invoke-UI (Wait-Enabled 'Review scope' $true)
  Invoke-UI (Wait-Enabled 'Start analysis' $true)
  $null = Wait-UI 'Scan finished' { Find-UI 'Scan finished' $textType } 120
  Invoke-UI (Wait-Enabled 'Browse folders' $true)
  $null = Wait-UI 'clip-0.mp4' { Find-UI 'clip-0.mp4' $null } 60
  Invoke-UI (Wait-Enabled 'Show space distribution' $true)
  $radioCondition = [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, [Windows.Automation.ControlType]::RadioButton)
  $barName = Get-UiName 'Bar chart'
  Select-SmokeRadio (Wait-UI 'Bar chart format' { @($window.FindAll($scopeTree, $radioCondition) | Where-Object { $_.Current.Name -like "$barName*" }) | Select-Object -First 1 } 60)
  $legendName = Get-UiName 'Legend'
  $listType = [Windows.Automation.ControlType]::List
  $listItem = [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, [Windows.Automation.ControlType]::ListItem)
  $legend = Wait-UI 'bar legend' {
    @($window.FindAll($scopeTree, [Windows.Automation.PropertyCondition]::new($ae::NameProperty, $legendName)) |
      Where-Object { $_.Current.ControlType -eq $listType }) | Select-Object -First 1
  } 60
  $videosName = Get-UiName 'Videos'
  $videos = Wait-UI 'Videos in the legend' { @($legend.FindAll($scopeTree, $listItem) | Where-Object { $_.Current.Name -like "$videosName*" }) | Select-Object -First 1 }
  $otherPrefix = (Get-UiName 'Other (7)') -replace '7.*$', ''
  $otherBar = { @($window.FindAll($scopeTree, $listItem) | Where-Object { $_.Current.Name -like "$otherPrefix*" }) | Select-Object -First 1 }
  $other = Wait-UI 'the Other bar' $otherBar 60
  "Unfiltered Other bar: '$($other.Current.Name)'"
  Save-SmokeCapture '01-bar-many'
  # The 20 bars run past the window: the Other bar and its break are at the bottom (README 47).
  Save-SmokeCapture '01b-bar-many-bottom' -Prepare { $null = Show-InWindow $other }
  Select-SmokeRadio $videos
  $clear = Wait-UI 'the type chip' { Find-TypeChip }
  $narrowed = Wait-UI 'the narrowed Other bar' {
    $bar = & $otherBar
    if ($bar -and $bar.Current.Name -ne $other.Current.Name) { $bar }
  } 60
  Save-SmokeCapture '02-bar-many-narrowed' -Prepare { $null = Show-InWindow $clear }
  Save-SmokeCapture '02b-bar-many-narrowed-bottom' -Prepare { $null = Show-InWindow $narrowed }
  "Bar many capture: $Locale, $Theme, window $(if ($WindowSize) { $WindowSize } else { 'default' })"
}
# F06 #320 (README 47): an Other no larger than the largest single bar is drawn to scale, unbroken.
function Test-CaptureBarOtherSmall {
  $scope = Join-Path $root 'bar-other-small-scope'
  [IO.Directory]::CreateDirectory($scope) | Out-Null
  [IO.File]::WriteAllBytes((Join-Path $scope 'big.iso'), [byte[]]::new(48MB))
  for ($index = 0; $index -lt 21; $index++) {
    [IO.File]::WriteAllBytes((Join-Path $scope ('small-{0:D2}.txt' -f $index)), [byte[]]::new(64KB + 4096 * $index))
  }
  Set-Scope $scope
  Invoke-UI (Wait-Enabled 'Review scope' $true)
  Invoke-UI (Wait-Enabled 'Start analysis' $true)
  $null = Wait-UI 'Scan finished' { Find-UI 'Scan finished' $textType } 60
  Invoke-UI (Wait-Enabled 'Browse folders' $true)
  $null = Wait-UI 'big.iso' { Find-UI 'big.iso' $null } 60
  Invoke-UI (Wait-Enabled 'Show space distribution' $true)
  $radioCondition = [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, [Windows.Automation.ControlType]::RadioButton)
  $barName = Get-UiName 'Bar chart'
  Select-SmokeRadio (Wait-UI 'Bar chart format' { @($window.FindAll($scopeTree, $radioCondition) | Where-Object { $_.Current.Name -like "$barName*" }) | Select-Object -First 1 } 60)
  $listItem = [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, [Windows.Automation.ControlType]::ListItem)
  $otherPrefix = (Get-UiName 'Other (7)') -replace '7.*$', ''
  $other = Wait-UI 'the Other bar' { @($window.FindAll($scopeTree, $listItem) | Where-Object { $_.Current.Name -like "$otherPrefix*" }) | Select-Object -First 1 } 60
  "Other bar: '$($other.Current.Name)'"
  Save-SmokeCapture '01-bar-other-small'
  Save-SmokeCapture '02-bar-other-small-bottom' -Prepare { $null = Show-InWindow $other }
  "Bar Other small capture: $Locale, $Theme, window $(if ($WindowSize) { $WindowSize } else { 'default' })"
}

function Test-CaptureTypeLegend([switch]$More) {
  $scope = Join-Path $root 'type-legend-scope'
  [IO.Directory]::CreateDirectory((Join-Path $scope 'tiny')) | Out-Null
  $files = [ordered]@{ 'report.pdf' = 6MB; 'photo.jpg' = 3MB; 'song.mp3' = 2MB; 'clip.mp4' = 8MB; 'backup.zip' = 4MB; 'ubuntu.iso' = 10MB; 'README' = 1MB; 'notes.xyz' = 2MB }
  foreach ($name in $files.Keys) { [IO.File]::WriteAllBytes((Join-Path $scope $name), [byte[]]::new($files[$name])) }
  # Drawn as one shape but too small for a header band, like the treemap capture's Tiny.
  foreach ($name in 'a.txt', 'b.txt') { [IO.File]::WriteAllBytes((Join-Path (Join-Path $scope 'tiny') $name), [byte[]]::new(256KB)) }
  # With -More, a junction is an item whose type cannot be decided: the legend's ninth class,
  # past the eight items shown before "more" (README 44.3 implementation check). A link puts
  # the folder's attribution in doubt, so these runs capture only the treemap legend.
  $link = Join-Path $scope 'link'
  if ($More) {
    $outside = Join-Path $root 'type-legend-outside'
    [IO.Directory]::CreateDirectory($outside) | Out-Null
    & cmd.exe /D /C mklink /J $link $outside | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Could not create the type legend junction' }
  }
  try {
  Set-Scope $scope
  Invoke-UI (Wait-Enabled 'Review scope' $true)
  Invoke-UI (Wait-Enabled 'Start analysis' $true)
  $null = Wait-UI 'Scan finished' { Find-UI 'Scan finished' $textType } 60
  Invoke-UI (Wait-Enabled 'Browse folders' $true)
  $null = Wait-UI 'report.pdf' { Find-UI 'report.pdf' $null }
  Invoke-UI (Wait-Enabled 'Show space distribution' $true)
  $legendName = Get-UiName 'Legend'
  $listType = [Windows.Automation.ControlType]::List
  $legend = Wait-UI 'type legend' {
    @($window.FindAll($scopeTree, [Windows.Automation.PropertyCondition]::new($ae::NameProperty, $legendName)) |
      Where-Object { $_.Current.ControlType -eq $listType }) | Select-Object -First 1
  } 60
  $listItem = [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, [Windows.Automation.ControlType]::ListItem)
  "Legend: $(@($legend.FindAll($scopeTree, $listItem) | ForEach-Object { $_.Current.Name }) -join ' | ')"
  "Folder note shown: $($null -ne (Find-UI 'Files in uncolored folders are counted in the legend' $null))"
  Save-SmokeCapture '01-treemap-type'
  if ($More) {
    $buttonCondition = [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, [Windows.Automation.ControlType]::Button)
    $open = Wait-UI 'the legend "more" button' { @($window.FindAll($scopeTree, $buttonCondition) | Where-Object { $_.Current.Name -match '1.*(more|더 보기)' }) | Select-Object -First 1 }
    Invoke-UI $open
    Start-Sleep -Milliseconds 800
    "Legend opened: $(@($legend.FindAll($scopeTree, $listItem)).Count) items"
    Save-SmokeCapture '02-legend-more-open'
    return
  }
  $radioType = [Windows.Automation.ControlType]::RadioButton
  $radioCondition = [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, $radioType)
  $format = { param($english) $wanted = Get-UiName $english; @($window.FindAll($scopeTree, $radioCondition) | Where-Object { $_.Current.Name -like "$wanted*" }) | Select-Object -First 1 }
  Select-SmokeRadio (Wait-UI 'Bar chart format' { & $format 'Bar chart' })
  $null = Wait-UI 'bar legend' { $window.FindFirst($scopeTree, [Windows.Automation.PropertyCondition]::new($ae::NameProperty, $legendName)) }
  Save-SmokeCapture '02-bar-type'
  Select-SmokeRadio (Wait-UI 'Treemap format' { & $format 'Treemap' })
  $legend = Wait-UI 'treemap legend' {
    @($window.FindAll($scopeTree, [Windows.Automation.PropertyCondition]::new($ae::NameProperty, $legendName)) |
      Where-Object { $_.Current.ControlType -eq $listType }) | Select-Object -First 1
  }
  $videosName = Get-UiName 'Videos'
  $videos = Wait-UI 'Videos in the legend' { @($legend.FindAll($scopeTree, $listItem) | Where-Object { $_.Current.Name -like "$videosName*" }) | Select-Object -First 1 }
  Select-SmokeRadio $videos
  "Legend class chosen: '$($videos.Current.Name)'"
  Save-SmokeCapture '03-legend-videos'
  # Choosing a type narrows the treemap to it (#301); the chip on the status line lets go.
  $chip = Wait-UI 'the type chip' { Find-TypeChip }
  "Class chip: '$($chip.Current.Name)'"
  Invoke-UI $chip
  Start-Sleep -Milliseconds 600
  $patterns = Wait-UI 'Use patterns' { Find-UI 'Use patterns' $null }
  # "Use patterns" is a toggle button: UI Automation offers it the Toggle pattern, not Invoke.
  $toggle = $null
  if (-not $patterns.TryGetCurrentPattern([Windows.Automation.TogglePattern]::Pattern, [ref]$toggle)) {
    throw "'$($patterns.Current.Name)' does not expose TogglePattern"
  }
  # In high contrast the switch is on and cannot be changed: patterns are always drawn.
  $forced = -not $patterns.Current.IsEnabled
  if (-not $forced) { $toggle.Toggle(); Start-Sleep -Milliseconds 800 }
  "Use patterns: $($toggle.Current.ToggleState), forced=$forced"
  Save-SmokeCapture '04-patterns'
  if (-not $forced) { $toggle.Toggle() }
  # "None" names more than one English text, so it is taken as the criterion group's first option.
  # The visible "Color by" label has the group's name too; the group is the one holding radios.
  $colorByName = Get-UiName 'Color by'
  $noneOption = Wait-UI 'Color by None' {
    @($window.FindAll($scopeTree, [Windows.Automation.PropertyCondition]::new($ae::NameProperty, $colorByName)) |
      ForEach-Object { @($_.FindAll($scopeTree, $radioCondition)) } | Where-Object { $_ }) | Select-Object -First 1
  }
  Select-SmokeRadio $noneOption
  Save-SmokeCapture '05-color-none'
  "Type legend capture: $Locale, $Theme, window $(if ($WindowSize) { $WindowSize } else { 'default' })"
  } finally {
    # Remove only the junction entry before the fixture is cleaned up.
    if ($More) { [IO.Directory]::Delete($link) }
  }
}
function Test-CaptureHistogramGaps {
  $scope = Join-Path $root 'histogram-gaps-scope'
  [IO.Directory]::CreateDirectory($scope) | Out-Null
  foreach ($index in 1..4) { [IO.File]::WriteAllBytes((Join-Path $scope "small-$index.bin"), [byte[]]::new(4KB)) }
  $large = [IO.File]::Create((Join-Path $scope 'large.bin'))
  try { $large.SetLength(1GB) } finally { $large.Dispose() }
  Set-Scope $scope
  Invoke-UI (Wait-Enabled 'Review scope' $true)
  Invoke-UI (Wait-Enabled 'Start analysis' $true)
  $null = Wait-UI 'Scan finished' { Find-UI 'Scan finished' $textType } 60
  Invoke-UI (Wait-Enabled 'Browse folders' $true)
  $null = Wait-UI 'large.bin' { Find-UI 'large.bin' $null }
  Invoke-UI (Wait-Enabled 'Show space distribution' $true)
  $radioType = [Windows.Automation.ControlType]::RadioButton
  $radioCondition = [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, $radioType)
  $wanted = Get-UiName 'Histogram'
  $radio = Wait-UI 'Histogram format' {
    $window.FindAll($scopeTree, $radioCondition) | Where-Object { $_.Current.Name -like "$wanted*" } | Select-Object -First 1
  }
  $pattern = $null
  $radio.SetFocus()
  if ($radio.TryGetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern, [ref]$pattern)) { $pattern.Select() }
  else { Invoke-UI $radio }
  $listItem = [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, [Windows.Automation.ControlType]::ListItem)
  $emptyName = { @($window.FindAll($scopeTree, $listItem) | Where-Object { $_.Current.Name -like '1 MiB*1 GiB, *' }) | Select-Object -First 1 }
  $empty = Wait-UI 'the empty 1 MiB–1 GiB interval' $emptyName 60
  "Empty interval on the item count axis: '$($empty.Current.Name)' enabled=$($empty.Current.IsEnabled)"
  Save-SmokeCapture '01-histogram-gaps-count'
  # The table below the chart: an empty interval has no open button, one with items opens them
  # as a range. The last interval's button brings the rows into the window.
  $buttonType = [Windows.Automation.ControlType]::Button
  $buttonCondition = [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, $buttonType)
  $openButton = { param($range) @($window.FindAll($scopeTree, $buttonCondition) | Where-Object { $_.Current.Name -like "*$range*" }) | Select-Object -First 1 }
  $openLast = Wait-UI 'the open button of the 1 GiB interval' { & $openButton '1 GiB*1 TiB' }
  $openEmpty = & $openButton '1 MiB*1 GiB'
  "Open buttons: 1 GiB–1 TiB shows '$($openLast.Current.Name)'; empty 1 MiB–1 GiB has one: $($null -ne $openEmpty)"
  Save-SmokeCapture '03-histogram-gaps-count-table' -Prepare { $null = Show-InWindow $openLast }
  $axisName = Get-UiName 'Vertical axis'
  $axis = Wait-UI 'Vertical axis' {
    $window.FindFirst($scopeTree, [Windows.Automation.PropertyCondition]::new($ae::NameProperty, $axisName))
  }
  # "Size" names more than one English text, so the size radio is taken as the group's second.
  $sizeAxis = @($axis.FindAll($scopeTree, $radioCondition))[1]
  $null = Show-InWindow $sizeAxis
  $sizeAxis.SetFocus()
  if ($sizeAxis.TryGetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern, [ref]$pattern)) { $pattern.Select() }
  else { Invoke-UI $sizeAxis }
  Start-Sleep -Milliseconds 2000
  if (Find-UI 'The chart could not be loaded.' $null) { throw 'The size axis request was refused' }
  $empty = Wait-UI 'the empty interval on the size axis' $emptyName 30
  "Empty interval on the size axis: '$($empty.Current.Name)'"
  Save-SmokeCapture '02-histogram-gaps-size'
  $openLast = Wait-UI 'the open button on the size axis' { & $openButton '1 GiB*1 TiB' }
  Save-SmokeCapture '04-histogram-gaps-size-table' -Prepare { $null = Show-InWindow $openLast }
  "Histogram gaps capture: $Locale, $Theme, window $(if ($WindowSize) { $WindowSize } else { 'default' })"
}
# F05 #133: the "Scan status" column of the browse table. A child folder the scan may not list reads
# "Contents not read" with a lower-bound size; a folder read normally and a file stay blank. The
# Other volume and Counted at another path marks cannot be made on a runner; the preview gallery
# captures them.
function Test-CaptureScanStatus {
  $scope = Join-Path $root 'scan-status-scope'
  $denied = Join-Path $scope 'denied-child'
  $plain = Join-Path $scope 'plain-child'
  New-Item -ItemType Directory -Path $denied, $plain -Force | Out-Null
  [IO.File]::WriteAllBytes((Join-Path $scope 'observed.bin'), [byte[]]::new(13))
  [IO.File]::WriteAllBytes((Join-Path $plain 'inside.bin'), [byte[]]::new(29))
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
    $null = Wait-UI 'Scan finished' { Find-UI 'Scan finished' $textType } 60
    Invoke-UI (Wait-Enabled 'Browse folders' $true)
    $null = Wait-UI 'Contents not read' { Find-UI 'Contents not read' $null } 60
    $null = Wait-UI 'plain-child' { Find-UI 'plain-child' $null }
    Save-SmokeCapture '01-scan-status'
  } finally { Set-Acl -LiteralPath $denied -AclObject $original }
  "Scan status capture: $Locale, $Theme, window $(if ($WindowSize) { $WindowSize } else { 'default' })"
}
function Test-CaptureTour {
  # #178: a name with a zero-width space and a direction mark. The table draws it as it is; the
  # expanded Full name in its details draws each one as its code point.
  $invisibleName = 'report' + [char]0x200B + 'final' + [char]0x200E + '.bin'
  [IO.File]::WriteAllBytes((Join-Path $scanRoot $invisibleName), [byte[]]::new(5))
  Set-Scope $scanRoot
  Invoke-UI (Wait-Enabled 'Review scope' $true)
  $null = Wait-Enabled 'Start analysis' $true
  Save-SmokeCapture '01-scope-reviewed'
  Invoke-UI (Wait-Enabled 'Start analysis' $true)
  $null = Wait-UI 'Scan finished' { Find-UI 'Scan finished' $textType }
  Save-SmokeCapture '02-analysis-result'
  # #199: the measurement rows carry descriptions; their values stay at the end of the row.
  Expand-UI (Wait-Enabled 'Measurement details' $true)
  $null = Wait-SmokeExpanded 'Measurement details' $true 'opening'
  Save-SmokeCapture '02b-measurement-details'
  $toggle = Wait-Enabled 'Measurement details' $true
  $disclosure = $null
  if ($toggle.TryGetCurrentPattern([Windows.Automation.ExpandCollapsePattern]::Pattern, [ref]$disclosure)) { $disclosure.Collapse() }
  else { Invoke-UI $toggle }
  $null = Wait-SmokeExpanded 'Measurement details' $false 'closing'
  Invoke-UI (Wait-Enabled 'Explain volume usage' $true)
  $null = Wait-UI 'volume explanation' { Find-UI 'Selected scanned folder' $textType }
  Save-SmokeCapture '03-volume-explanation' -Prepare { Show-VolumeExplanation }
  Invoke-UI (Wait-Enabled 'Hide volume explanation' $true)
  Invoke-UI (Wait-Enabled 'View folder details' $true)
  Wait-Details 'Space measurement details' 'Observed item space'
  $null = Wait-UI 'folder details' { Find-UI 'Observed item space' $null }
  Save-SmokeCapture '04-folder-details'
  Invoke-UI (Wait-Enabled 'Close details' $true)
  Invoke-UI (Wait-Enabled 'Browse folders' $true)
  $null = Wait-UI 'root.bin' { Find-UI 'root.bin' $null }
  Save-SmokeCapture '05-browse-folders'
  # #145: the location path breaks after its separators on screen; the names UI Automation reads
  # for it should still be the path itself, without spaces at the breaks.
  $leaf = '*\' + (Split-Path -Leaf $scanRoot)
  "Browse location names: $((@(Get-UINames | Where-Object { $_ -like $leaf })) -join ' | ')"
  Invoke-UI (Wait-Enabled 'root.bin' $true)
  $null = Wait-UI 'file details' { Find-UI 'File details' $textType }
  Save-SmokeCapture '06-file-details'
  Invoke-UI (Wait-Enabled 'Close details' $true)
  $invisibleRow = Wait-UI 'name with invisible characters' {
    $window.FindAll($scopeTree, [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, $button)) |
      Where-Object { ($_.Current.Name -replace '[\u200B\u200E]', '') -eq 'reportfinal.bin' -and $_.Current.IsEnabled } |
      Select-Object -First 1
  }
  Invoke-UI $invisibleRow
  $null = Wait-UI 'file details' { Find-UI 'File details' $textType }
  Expand-UI (Wait-Enabled 'Full name' $true)
  $null = Wait-SmokeExpanded 'Full name' $true 'opening'
  Save-SmokeCapture '07-invisible-name'
  Invoke-UI (Wait-Enabled 'Close details' $true)
  # F15 README 1: a grade alone puts no notice at the top. The runner's startup grade is not a
  # pass, so the support entry carries the dot until support information is opened.
  if (Find-UI 'Startup notice' $null) { throw 'A startup notice is shown above the first screen' }
  $supportEntry = Wait-UI 'support entry with items to review' { Find-UI 'Support information, items to review' $button }
  Save-SmokeCapture '08-support-entry-dot'
  Invoke-UI $supportEntry
  $null = Wait-UI 'startup checks section' { Find-UI 'Startup checks' $null }
  Save-SmokeCapture '09-startup-checks'
  Invoke-UI (Wait-Enabled 'Close details' $true)
  $null = Wait-UI 'support entry without the dot' { Find-UI 'Support information' $button }
  # #199: the product screen's build and install rows carry descriptions.
  Invoke-UI (Wait-Enabled 'About DiskUnfold' $true)
  $null = Wait-UI 'product information' { Find-UI 'Close details' $button }
  Start-Sleep -Milliseconds 800
  Save-SmokeCapture '10-product-info'
  Invoke-UI (Wait-Enabled 'Close details' $true)
  "Capture tour: $Locale, $Theme, window $(if ($WindowSize) { $WindowSize } else { 'default' })"
}
# F02 #170: the operation list of the operation history, with the Operation column that no longer
# shows the engine's target text under the operation name. The names are found through the bundle
# of the display language, as the capture profile is not switched to English.
function Test-CaptureHistory {
  Set-Scope $scanRoot
  Invoke-UI (Wait-Enabled 'Review scope' $true)
  Invoke-UI (Wait-Enabled 'Start analysis' $true)
  $null = Wait-UI 'Scan finished' { Find-UI 'Scan finished' $textType }
  Expand-UI (Wait-TasksEntry)
  $null = Wait-UI 'recently finished job section' {
    Find-UI 'Recently finished' $textType
  }
  Save-SmokeCapture '08-job-center-recently-finished'
  # F24 #289 scene 7 uses the gallery: the small smoke scan finishes before its header count can be captured.
  Send-SmokeKey 0x1B
  $focusedTaskEntry = Wait-UI 'job center Escape focus restoration' {
    $focused = $ae::FocusedElement
    $taskName = Get-UiName 'Tasks'
    if (
      $focused -and
      ($focused.Current.Name -eq $taskName -or $focused.Current.Name.StartsWith($taskName + ', '))
    ) { $focused }
  }
  "F24 #289 job center focus: $($focusedTaskEntry.Current.Name)"
  Expand-UI (Wait-TasksEntry)
  Invoke-UI (Wait-VisibleAction 'View all task history')
  # The row's "View details: <operation>, <time>" button. Its translated template cannot be looked
  # up by its English text (a shorter "{label}: {value}" template matches first), so the button is
  # found by the translated operation name between the colon and the time.
  $details = '*: ' + (Get-UiName 'Disk analysis') + ', *'
  $null = Wait-UI 'recorded disk analysis' {
    $window.FindAll($scopeTree, [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, $button)) |
      Where-Object { $_.Current.IsEnabled -and $_.Current.Name -like $details } |
      Select-Object -First 1
  }
  Save-SmokeCapture '01-operation-history'
  $retentionSettingsName = Get-UiName 'Retention'
  $retentionEntry = Wait-VisibleAction $retentionSettingsName
  Expand-UI $retentionEntry
  $retentionTitleName = Get-UiName 'History retention'
  $null = Wait-UI 'history retention panel title' {
    Find-UI $retentionTitleName $null
  }
  Save-SmokeCapture '09-retention-panel'
  $autoCleanupName = Get-UiName 'Clean up automatically'
  $findAutoCleanup = {
    $nameCondition = [Windows.Automation.PropertyCondition]::new($ae::NameProperty, $autoCleanupName)
    $matches = $window.FindAll($scopeTree, $nameCondition)
    foreach ($item in $matches) {
      $pattern = $null
      if ($item.TryGetCurrentPattern([Windows.Automation.TogglePattern]::Pattern, [ref]$pattern)) { return $item }
    }
    foreach ($item in $matches) {
      $pattern = $null
      if ($item.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) { return $item }
    }
  }
  $autoCleanup = Wait-UI 'automatic cleanup switch' {
    & $findAutoCleanup
  }
  $togglePattern = $null
  if ($autoCleanup.TryGetCurrentPattern([Windows.Automation.TogglePattern]::Pattern, [ref]$togglePattern)) {
    if ($togglePattern.Current.ToggleState -ne [Windows.Automation.ToggleState]::Off) {
      throw 'Automatic cleanup was already on before the retention review'
    }
    $togglePattern.Toggle()
  } else {
    Invoke-UI $autoCleanup
  }
  $confirmationName = Get-UiName 'Turn on automatic cleanup?'
  $null = Wait-UI 'automatic cleanup confirmation dialog' {
    Find-UI $confirmationName $null
  }
  Save-SmokeCapture '10-retention-enable-confirmation'
  $cancelName = Get-UiName 'Cancel'
  $focusDeadline = [DateTime]::UtcNow.AddSeconds(3)
  do {
    $focused = $ae::FocusedElement
    $focusedName = if ($focused) { $focused.Current.Name } else { '' }
    if ($focusedName -eq $cancelName) { break }
    Start-Sleep -Milliseconds 80
  } while ([DateTime]::UtcNow -lt $focusDeadline)
  "F21 #316 retention confirm focus: $focusedName"
  if ($focusedName -ne $cancelName) {
    throw "Retention confirmation focused '$focusedName' instead of '$cancelName'"
  }
  Send-SmokeKey 0x1B
  $null = Wait-UI 'retention confirmation dismissed' {
    -not (Find-UI $confirmationName $null)
  }
  $autoCleanup = Wait-UI 'automatic cleanup switch after cancellation' {
    & $findAutoCleanup
  }
  $togglePattern = $null
  if (-not $autoCleanup.TryGetCurrentPattern([Windows.Automation.TogglePattern]::Pattern, [ref]$togglePattern)) {
    throw 'Automatic cleanup did not expose TogglePattern to verify it remained off'
  }
  if ($togglePattern.Current.ToggleState -ne [Windows.Automation.ToggleState]::Off) {
    throw 'Automatic cleanup remained on after cancelling the confirmation'
  }
  $filterDisclosure = Wait-UI 'date and type filters disclosure' {
    $item = Find-UI 'Date and type filters' $button
    if ($item -and $item.Current.IsEnabled) { $item }
  }
  Expand-UI $filterDisclosure
  $startDateLabel = Get-UiName 'From date'
  $throughDateLabel = Get-UiName 'Through date'
  # Fill the template with the already localized label; the template keeps placeholder text as given.
  $startDateCalendarName = Get-UiName "Open calendar for $startDateLabel"
  $null = Wait-UI 'history date fields' {
    if ((Find-UI $startDateLabel $null) -and (Find-UI $throughDateLabel $null)) { $true }
  }
  Save-SmokeCapture '02-operation-history-filters'
  $startDateCalendarButton = Wait-UI 'start-date calendar button' {
    $item = Find-UI $startDateCalendarName $button
    if (-not $item) { $item = Find-UI 'Show date picker' $button }
    $item
  }
  $buttonName = $startDateCalendarButton.Current.Name
  $buttonHelpText = $startDateCalendarButton.Current.HelpText
  $buttonLog = "F21 #272 date picker button: $buttonName"
  if (-not [string]::IsNullOrWhiteSpace($buttonHelpText)) { $buttonLog += "; HelpText: $buttonHelpText" }
  $buttonLog
  if ([string]::IsNullOrWhiteSpace($buttonName) -or $buttonName -eq 'Show date picker') {
    throw "Unexpected start-date calendar button Name: '$buttonName'"
  }
  "History capture: $Locale, $Theme, window $(if ($WindowSize) { $WindowSize } else { 'default' })"
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
  # The first start on a runner also starts WebView2 cold; its page has been seen to
  # take over 22 s to show its first control (#281, empty UI tree at the timeout).
  $startupLanguage = Wait-UI 'initial locale' {
    if (Find-UI '언어 및 표시' $button) { 'ko' }
    elseif (Find-UI 'Language and display' $button) { 'en' }
  } 60
  $captureOnly = $CaptureTour -or $CaptureTreemap -or $CaptureFallbackTable -or $CaptureTreemapLink -or $CaptureProgress -or $CaptureUniquePhysical -or $CaptureHistory -or $CaptureScanStatus -or $CaptureHistogramGaps -or $CaptureTypeLegend -or $CaptureTypeLegendMore -or $CaptureTypeNarrowing -or $CaptureTypeNarrowingShort -or $CaptureTypeNarrowingMany -or $CaptureBarOtherSmall -or $CaptureTypeList -or $CaptureTypeListShort
  if ($startupLanguage -eq 'ko' -and -not $captureOnly) {
    Select-SmokeLanguage '언어 및 표시' '표시 언어' 'English' 'Display language' 'Close'
  }
  $null = Wait-UI 'path field' { Find-UI 'Folder path' $edit }
  if ($CaptureTour) { Test-CaptureTour }
  if ($CaptureTreemap) { Test-CaptureTreemap }
  if ($CaptureFallbackTable) { Test-CaptureFallbackTable }
  if ($CaptureTreemapLink) { Test-CaptureTreemapLink }
  if ($CaptureProgress) { Test-CaptureProgress }
  if ($CaptureUniquePhysical) { Test-CaptureUniquePhysical }
  if ($CaptureHistogramGaps) { Test-CaptureHistogramGaps }
  if ($CaptureTypeLegend) { Test-CaptureTypeLegend }
  if ($CaptureTypeLegendMore) { Test-CaptureTypeLegend -More }
  if ($CaptureTypeNarrowing) { Test-CaptureTypeNarrowing }
  if ($CaptureTypeNarrowingShort) { Test-CaptureTypeNarrowing -Short }
  if ($CaptureTypeNarrowingMany) { Test-CaptureTypeNarrowingMany }
  if ($CaptureTypeList) { Test-CaptureTypeNarrowing -List }
  if ($CaptureTypeListShort) { Test-CaptureTypeNarrowing -List -Short }
  if ($CaptureBarOtherSmall) { Test-CaptureBarOtherSmall }
  if ($CaptureHistory) { Test-CaptureHistory }
  if ($CaptureScanStatus) { Test-CaptureScanStatus }
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
    $null = Wait-UI 'target editor collapsed after accepted scan' { -not (Find-UI 'Folder path' $edit) }
    Set-Scope $child
    Invoke-UI (Wait-VisibleAction 'Review scope')
    $null = Wait-Enabled 'Start analysis' $true
    Invoke-UI (Wait-VisibleAction 'Cancel selection')
    $null = Wait-UI 'discarded scope and retained final total' {
      $field = Find-UI 'Folder path' $edit
      if ($field -and (Find-ResultVersionLabel 'Final') -and
          $field.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).Current.Value -eq '') {
        Find-LogicalSize '18 bytes'
      }
    }
    $null = Wait-Enabled 'Start analysis' $false
    'F01 native selection cancellation: discarded reviewed scope and retained the final 18-byte result'
    Invoke-UI (Wait-VisibleAction 'Close target editor')
    $null = Wait-UI 'target editor closed with the result still visible' {
      if (-not (Find-UI 'Folder path' $edit)) {
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
  if ($Theme -eq 'contrast' -and -not $originalContrast) { $null = [SmokeContrast]::Set($false) }
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



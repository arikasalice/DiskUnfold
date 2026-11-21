#Requires -Version 7.4
# Runs the Windows UI smoke scenarios listed in windows-ui-smoke.json, one isolated
# smoke-windows.ps1 invocation per scenario, and keeps each scenario's log and review captures.
# A scenario has a unique lower-case name, a tier ("smoke" or "gallery"), the switches of
# smoke-windows.ps1 to pass ("arguments"), and optionally "locale" (en or ko), "theme" (light,
# dark or contrast), "windowSize" (WIDTHxHEIGHT) and "appScale" (125, 150 or 200, the app's CSS
# zoom; the runner's display scale stays 100%).
# Captures go to a folder named <name>-<locale>-<theme>-<size>, with -scale<n> when the app scale
# is not 100. Use -Only to run specific scenarios, or -Tier smoke to run the PR smoke subset.
# -Repeat runs each scenario that many times, each run in its own folder and log with -run<n>, to
# look for intermittent failures.
# -ScreenSize switches the display to a mode at least that large first, for window sizes the
# default CI screen cannot hold. Exits 1 when any scenario failed.
[CmdletBinding()]
param(
  [Parameter(Mandatory)][string]$Binary,
  [Parameter(Mandatory)][string]$OutputDirectory,
  [string]$ScenarioFile = (Join-Path $PSScriptRoot 'windows-ui-smoke.json'),
  [string[]]$Only = @(),
  [ValidateSet('all', 'smoke')][string]$Tier = 'all',
  [ValidateRange(1, 20)][int]$Repeat = 1,
  [string]$ScreenSize
)
$ErrorActionPreference = 'Stop'
if (-not ('SmokeDisplay' -as [type])) {
  Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

public static class SmokeDisplay {
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Ansi)]
  private struct DevMode {
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string DeviceName;
    public short SpecVersion, DriverVersion, Size, DriverExtra;
    public int Fields, PositionX, PositionY, DisplayOrientation, DisplayFixedOutput;
    public short Color, Duplex, YResolution, TTOption, Collate;
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string FormName;
    public short LogPixels;
    public int BitsPerPel, PelsWidth, PelsHeight, DisplayFlags, DisplayFrequency, IcmMethod, IcmIntent, MediaType, DitherType, Reserved1, Reserved2, PanningWidth, PanningHeight;
  }

  [DllImport("user32.dll", CharSet = CharSet.Ansi)]
  private static extern bool EnumDisplaySettings(string device, int mode, ref DevMode devMode);
  [DllImport("user32.dll", CharSet = CharSet.Ansi)]
  private static extern int ChangeDisplaySettings(ref DevMode devMode, int flags);

  private static DevMode Blank() {
    var mode = new DevMode();
    mode.Size = (short)Marshal.SizeOf(typeof(DevMode));
    return mode;
  }

  // Distinct width x height pairs the display driver offers.
  public static string[] Modes() {
    var seen = new SortedSet<string>();
    var mode = Blank();
    for (int index = 0; EnumDisplaySettings(null, index, ref mode); index++) seen.Add(mode.PelsWidth + "x" + mode.PelsHeight);
    return new List<string>(seen).ToArray();
  }

  public static string Current() {
    var mode = Blank();
    return EnumDisplaySettings(null, -1, ref mode) ? mode.PelsWidth + "x" + mode.PelsHeight : "unknown";
  }

  // Returns 0 when the display now uses the size.
  public static int Set(int width, int height) {
    var best = Blank();
    var found = false;
    var mode = Blank();
    for (int index = 0; EnumDisplaySettings(null, index, ref mode); index++) {
      if (mode.PelsWidth == width && mode.PelsHeight == height && mode.BitsPerPel >= 24 && (!found || mode.DisplayFrequency > best.DisplayFrequency)) { best = mode; found = true; }
    }
    if (!found) return -100;
    best.Fields = 0x80000 | 0x100000 | 0x40000;
    return ChangeDisplaySettings(ref best, 0);
  }
}
'@
}
if ($ScreenSize) {
  $modes = [SmokeDisplay]::Modes()
  "Display now $([SmokeDisplay]::Current()); modes: $($modes -join ' ')"
  $wanted = $ScreenSize
  if ($modes -notcontains $wanted) {
    $wanted = $modes | Where-Object { $_ -match '^(\d+)x(\d+)$' -and [int]$Matches[1] -ge 1440 -and [int]$Matches[2] -ge 900 } |
      Sort-Object { [int](($_ -split 'x')[0]) * [int](($_ -split 'x')[1]) } | Select-Object -First 1
  }
  if (-not $wanted) { throw "No display mode of at least 1440x900 is available (modes: $($modes -join ' '))" }
  if ([SmokeDisplay]::Current() -ne $wanted) {
    $size = $wanted -split 'x'
    $code = [SmokeDisplay]::Set([int]$size[0], [int]$size[1])
    if ($code -ne 0) { throw "Could not switch the display to $wanted (code $code)" }
    Start-Sleep -Milliseconds 500
    "Display switched to $([SmokeDisplay]::Current())"
  }
}
# Clean up any stale DiskUnfold processes or temporary smoke fixtures left by a cancelled run on a
# persistent self-hosted runner.
Get-Process -Name diskunfold -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Get-ChildItem -LiteralPath ([IO.Path]::GetTempPath()) -Directory -Filter 'diskunfold-smoke-*' -ErrorAction SilentlyContinue |
  Remove-Item -Recurse -Force -ErrorAction SilentlyContinue

$smoke = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\launcher\smoke-windows.ps1')).Path
$exe = (Resolve-Path -LiteralPath $Binary).Path
$scenarios = @((Get-Content -Raw -LiteralPath $ScenarioFile | ConvertFrom-Json).scenarios)
if ($Only.Count) {
  $scenarios = @($scenarios | Where-Object { $Only -contains $_.name })
  if (-not $scenarios.Count) { throw "No scenario matches: $($Only -join ', ')" }
} elseif ($Tier -eq 'smoke') {
  $scenarios = @($scenarios | Where-Object { ($null -eq $_.tier) -or ($_.tier -eq 'smoke') })
  if (-not $scenarios.Count) { throw "No scenario matches tier '$Tier'" }
}
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$out = (Resolve-Path -LiteralPath $OutputDirectory).Path
$runs = foreach ($scenario in $scenarios) {
  foreach ($run in 1..$Repeat) { [pscustomobject]@{ Scenario = $scenario; Run = $run } }
}
$sharedCache = Join-Path ([IO.Path]::GetTempPath()) ('diskunfold-smoke-cache-' + [guid]::NewGuid())
New-Item -ItemType Directory -Force -Path $sharedCache | Out-Null
$env:DISKUNFOLD_SMOKE_CACHE_DIR = $sharedCache
try {
  $results = foreach ($entry in $runs) {
    $scenario = $entry.Scenario
    $runSuffix = if ($Repeat -gt 1) { "-run$($entry.Run)" } else { '' }
    $locale = if ($scenario.locale) { $scenario.locale } else { 'en' }
    $theme = if ($scenario.theme) { $scenario.theme } else { 'light' }
    $size = if ($scenario.windowSize) { $scenario.windowSize } else { 'default' }
    $scale = if ($scenario.appScale) { [string]$scenario.appScale } else { '100' }
    $folderName = "$($scenario.name)-$locale-$theme-$size" + $(if ($scale -ne '100') { "-scale$scale" } else { '' }) + $runSuffix
    $captures = Join-Path $out $folderName
    New-Item -ItemType Directory -Force -Path $captures | Out-Null
    $log = Join-Path $out "$($scenario.name)$runSuffix.log"
    $started = Get-Date
    $arguments = @{
      Binary = $exe
      CaptureDirectory = $captures
      KeepFixture = $true
      Locale = $locale
      Theme = $theme
    }
    if ($scenario.windowSize) { $arguments['WindowSize'] = $scenario.windowSize }
    if ($scale -ne '100') { $arguments['AppScale'] = $scale }
    foreach ($switchArg in @($scenario.arguments)) {
      $switchName = $switchArg.TrimStart('-')
      if ($switchName) { $arguments[$switchName] = $true }
    }
    $passed = $true
    try {
      & $smoke @arguments *>&1 | Tee-Object -FilePath $log | Out-Host
    } catch {
      $_ | Out-String | Tee-Object -FilePath $log -Append | Out-Host
      $passed = $false
      Get-Process -Name diskunfold -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    }
    [pscustomobject]@{
      name = $scenario.name
      folder = $folderName
      passed = $passed
      seconds = [int]((Get-Date) - $started).TotalSeconds
      captures = @(Get-ChildItem -LiteralPath $captures -Filter *.png | ForEach-Object Name)
    }
  }
} finally {
  Remove-Item Env:\DISKUNFOLD_SMOKE_CACHE_DIR -ErrorAction SilentlyContinue
  Remove-Item -LiteralPath $sharedCache -Recurse -Force -ErrorAction SilentlyContinue
  Get-Process -Name diskunfold, msedgewebview2 -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
  Get-ChildItem -LiteralPath ([IO.Path]::GetTempPath()) -Directory -Filter 'diskunfold-smoke-*' -ErrorAction SilentlyContinue |
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
}
$results | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $out 'summary.json') -Encoding utf8
$lines = @('| Scenario | Folder | Result | Seconds | Captures |', '| --- | --- | --- | --- | --- |')
$lines += $results | ForEach-Object { "| $($_.name) | $($_.folder) | $(if ($_.passed) { 'pass' } else { 'FAIL' }) | $($_.seconds) | $($_.captures.Count) |" }
$lines | Out-Host
if ($env:GITHUB_STEP_SUMMARY) { $lines | Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY }
if ($results | Where-Object { -not $_.passed }) { exit 1 }

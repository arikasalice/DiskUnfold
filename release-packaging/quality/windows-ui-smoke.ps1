#Requires -Version 7.4
# Runs the Windows UI smoke scenarios listed in windows-ui-smoke.json, one fresh smoke-windows.ps1
# process per scenario with isolated data, and keeps each scenario's log and review captures.
# A scenario has a unique lower-case name, the switches of smoke-windows.ps1 to pass ("arguments"),
# and optionally "locale" (en or ko), "theme" (light or dark), "windowSize" (WIDTHxHEIGHT) and
# "appScale" (125, 150 or 200, the app's CSS zoom; the runner's display scale stays 100%).
# Captures go to a folder named <name>-<locale>-<theme>-<size>, with -scale<n> when the app scale
# is not 100. Use -Only to run some scenarios.
# -Repeat runs each scenario that many times, each run in its own folder and log with -run<n>, to
# look for intermittent failures.
# -ScreenSize switches the display to a mode at least that large first, for window sizes the
# default CI screen cannot hold. Exits 1 when any scenario failed.
# -Shard and -ShardCount partition the filtered scenarios into deterministic shards.
[CmdletBinding()]
param(
  [Parameter(Mandatory)][string]$Binary,
  [Parameter(Mandatory)][string]$OutputDirectory,
  [string]$ScenarioFile = (Join-Path $PSScriptRoot 'windows-ui-smoke.json'),
  [string[]]$Only = @(),
  [ValidateRange(1, 20)][int]$ShardCount = 1,
  [ValidateRange(1, 20)][int]$Shard = 1,
  [ValidateRange(1, 20)][int]$Repeat = 1,
  [string]$ScreenSize
)
$ErrorActionPreference = 'Stop'
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
if ($ScreenSize) {
  $modes = [SmokeDisplay]::Modes()
  "Display now $([SmokeDisplay]::Current()); modes: $($modes -join ' ')"
  $wanted = $ScreenSize
  if ($modes -notcontains $wanted) {
    $wanted = $modes | Where-Object { $_ -match '^(\d+)x(\d+)$' -and [int]$Matches[1] -ge 1440 -and [int]$Matches[2] -ge 900 } |
      Sort-Object { [int](($_ -split 'x')[0]) * [int](($_ -split 'x')[1]) } | Select-Object -First 1
  }
  if (-not $wanted) { throw "No display mode of at least 1440x900 is available (modes: $($modes -join ' '))" }
  $size = $wanted -split 'x'
  $code = [SmokeDisplay]::Set([int]$size[0], [int]$size[1])
  if ($code -ne 0) { throw "Could not switch the display to $wanted (code $code)" }
  Start-Sleep -Seconds 2
  "Display switched to $([SmokeDisplay]::Current())"
}
$smoke = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\launcher\smoke-windows.ps1')).Path
$exe = (Resolve-Path -LiteralPath $Binary).Path
$scenarios = @((Get-Content -Raw -LiteralPath $ScenarioFile | ConvertFrom-Json).scenarios)
if ($Only.Count) {
  $scenarios = @($scenarios | Where-Object { $Only -contains $_.name })
  if (-not $scenarios.Count) { throw "No scenario matches: $($Only -join ', ')" }
}
if ($Shard -gt $ShardCount) { throw "Shard $Shard exceeds shard count $ShardCount" }
$scenarios = @(
  for ($index = 0; $index -lt $scenarios.Count; $index++) {
    if (($index % $ShardCount) -eq ($Shard - 1)) { $scenarios[$index] }
  }
)
if (-not $scenarios.Count) { throw "No scenarios assigned to shard $Shard of $ShardCount" }
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$out = (Resolve-Path -LiteralPath $OutputDirectory).Path
$runs = foreach ($scenario in $scenarios) {
  foreach ($run in 1..$Repeat) { [pscustomobject]@{ Scenario = $scenario; Run = $run } }
}
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
  $arguments = @($scenario.arguments) + @('-Locale', $locale, '-Theme', $theme)
  if ($scenario.windowSize) { $arguments += @('-WindowSize', $scenario.windowSize) }
  if ($scale -ne '100') { $arguments += @('-AppScale', $scale) }
  & pwsh -NoProfile -File $smoke -Binary $exe -CaptureDirectory $captures -KeepFixture @arguments *>&1 | Tee-Object -FilePath $log | Out-Host
  [pscustomobject]@{
    name = $scenario.name
    folder = $folderName
    passed = ($LASTEXITCODE -eq 0)
    seconds = [int]((Get-Date) - $started).TotalSeconds
    captures = @(Get-ChildItem -LiteralPath $captures -Filter *.png | ForEach-Object Name)
  }
}
$results | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $out 'summary.json') -Encoding utf8
$lines = @('| Scenario | Folder | Result | Seconds | Captures |', '| --- | --- | --- | --- | --- |')
$lines += $results | ForEach-Object { "| $($_.name) | $($_.folder) | $(if ($_.passed) { 'pass' } else { 'FAIL' }) | $($_.seconds) | $($_.captures.Count) |" }
$lines | Out-Host
if ($env:GITHUB_STEP_SUMMARY) { $lines | Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY }
if ($results | Where-Object { -not $_.passed }) { exit 1 }

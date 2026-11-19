#Requires -Version 7.4
# Runs the Windows UI smoke scenarios listed in windows-ui-smoke.json, one fresh smoke-windows.ps1
# process per scenario with isolated data, and keeps each scenario's log and review captures.
# Add a scenario by adding an entry to the JSON list: a unique lower-case name and the switches of
# smoke-windows.ps1 to pass. Use -Only to run some of them. Exits 1 when any scenario failed.
[CmdletBinding()]
param(
  [Parameter(Mandatory)][string]$Binary,
  [Parameter(Mandatory)][string]$OutputDirectory,
  [string]$ScenarioFile = (Join-Path $PSScriptRoot 'windows-ui-smoke.json'),
  [string[]]$Only = @()
)
$ErrorActionPreference = 'Stop'
$smoke = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\launcher\smoke-windows.ps1')).Path
$exe = (Resolve-Path -LiteralPath $Binary).Path
$scenarios = @((Get-Content -Raw -LiteralPath $ScenarioFile | ConvertFrom-Json).scenarios)
if ($Only.Count) {
  $scenarios = @($scenarios | Where-Object { $Only -contains $_.name })
  if (-not $scenarios.Count) { throw "No scenario matches: $($Only -join ', ')" }
}
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$out = (Resolve-Path -LiteralPath $OutputDirectory).Path
$results = foreach ($scenario in $scenarios) {
  $captures = Join-Path $out $scenario.name
  New-Item -ItemType Directory -Force -Path $captures | Out-Null
  $log = Join-Path $out "$($scenario.name).log"
  $started = Get-Date
  $arguments = @($scenario.arguments)
  & pwsh -NoProfile -File $smoke -Binary $exe -CaptureDirectory $captures -KeepFixture @arguments *>&1 | Tee-Object -FilePath $log | Out-Host
  [pscustomobject]@{
    name = $scenario.name
    passed = ($LASTEXITCODE -eq 0)
    seconds = [int]((Get-Date) - $started).TotalSeconds
    captures = @(Get-ChildItem -LiteralPath $captures -Filter *.png | ForEach-Object Name)
  }
}
$results | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $out 'summary.json') -Encoding utf8
$lines = @('| Scenario | Result | Seconds | Captures |', '| --- | --- | --- | --- |')
$lines += $results | ForEach-Object { "| $($_.name) | $(if ($_.passed) { 'pass' } else { 'FAIL' }) | $($_.seconds) | $($_.captures.Count) |" }
$lines | Out-Host
if ($env:GITHUB_STEP_SUMMARY) { $lines | Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY }
if ($results | Where-Object { -not $_.passed }) { exit 1 }

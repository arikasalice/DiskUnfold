# Exercise the native smoke cleanup function without starting DiskUnfold.
# Run with: pwsh -NoProfile -File release-packaging/launcher/tests/smoke-windows-cleanup.ps1
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

function Assert-Condition([bool]$Condition, [string]$Message) {
  if (-not $Condition) { throw $Message }
}

function New-SmokeCleanupFixture {
  $root = Join-Path ([IO.Path]::GetTempPath()) ('diskunfold-smoke-' + [guid]::NewGuid())
  $lockDirectory = Join-Path $root 'profile\DiskUnfold\webview\EBWebView'
  [IO.Directory]::CreateDirectory($lockDirectory) | Out-Null
  $lockPath = Join-Path $lockDirectory 'lockfile'
  [IO.File]::WriteAllText($lockPath, 'synthetic smoke profile lock')
  [IO.File]::WriteAllText((Join-Path $root 'stdout.txt'), 'preserved smoke stdout')
  [IO.File]::WriteAllText((Join-Path $root 'stderr.txt'), '')
  [pscustomobject]@{
    Root = $root
    LockPath = $lockPath
    ReadyPath = Join-Path $root 'lock-ready.txt'
    ReleasePath = Join-Path $root 'lock-release.txt'
  }
}

function Start-SmokeLockJob($fixture, [int]$ReleaseAfterSeconds) {
  Start-Job -ArgumentList $fixture.LockPath, $fixture.ReadyPath, $fixture.ReleasePath, $ReleaseAfterSeconds -ScriptBlock {
    param($LockPath, $ReadyPath, $ReleasePath, $ReleaseAfterSeconds)
    $stream = [IO.File]::Open($LockPath, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    try {
      [IO.File]::WriteAllText($ReadyPath, 'held')
      if ($ReleaseAfterSeconds -ge 0) {
        Start-Sleep -Seconds $ReleaseAfterSeconds
      } else {
        while (-not [IO.File]::Exists($ReleasePath)) { Start-Sleep -Milliseconds 50 }
      }
    } finally {
      $stream.Dispose()
    }
  }
}

function Wait-SmokeLockReady($job, $fixture) {
  $deadline = [DateTime]::UtcNow.AddSeconds(10)
  while (-not (Test-Path -LiteralPath $fixture.ReadyPath)) {
    if ($job.State -in @('Failed', 'Stopped', 'Completed')) {
      throw "The synthetic lock job ended before acquiring its lock: $($job.State)"
    }
    if ([DateTime]::UtcNow -ge $deadline) { throw 'Timed out waiting for the synthetic lock job' }
    Start-Sleep -Milliseconds 50
  }
}

function Complete-SmokeLockJob($job, $fixture) {
  if (-not $job) { return }
  if (($job.State -eq 'Running' -or $job.State -eq 'NotStarted') -and
      (Test-Path -LiteralPath $fixture.Root) -and
      -not (Test-Path -LiteralPath $fixture.ReleasePath)) {
    [IO.File]::WriteAllText($fixture.ReleasePath, 'release')
  }
  $null = Wait-Job -Job $job -Timeout 10
  if ($job.State -eq 'Running' -or $job.State -eq 'NotStarted') {
    Stop-Job -Job $job
    $null = Wait-Job -Job $job -Timeout 5
  }
  Remove-Job -Job $job -Force
}

function Remove-TestFixture([string]$Path) {
  if (-not (Test-Path -LiteralPath $Path)) { return }
  $temp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([IO.Path]::DirectorySeparatorChar)
  $resolved = (Resolve-Path -LiteralPath $Path).Path
  if (-not [string]::Equals([IO.Path]::GetDirectoryName($resolved), $temp, [StringComparison]::OrdinalIgnoreCase) -or
      [IO.Path]::GetFileName($resolved) -notmatch '^diskunfold-smoke-[0-9a-f-]{36}$') {
    throw "Refusing to remove unexpected test fixture: $resolved"
  }
  Microsoft.PowerShell.Management\Remove-Item -LiteralPath $resolved -Recurse -Force
}

$smokeScript = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\smoke-windows.ps1')).Path
$tokens = $null
$parseErrors = $null
$smokeAst = [Management.Automation.Language.Parser]::ParseFile($smokeScript, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors) { throw "Smoke script has PowerShell parse errors: $($parseErrors[0].Message)" }
$cleanupAst = $smokeAst.Find({
  param($node)
  $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Remove-SmokeFixture'
}, $true)
if (-not $cleanupAst) { throw 'Could not find Remove-SmokeFixture in the smoke script' }
. ([scriptblock]::Create($cleanupAst.Extent.Text))

$releaseFixture = $null
$releaseJob = $null
$persistentFixture = $null
$persistentJob = $null
$deniedFixture = $null
try {
  $releaseFixture = New-SmokeCleanupFixture
  $releaseJob = Start-SmokeLockJob $releaseFixture 3
  Wait-SmokeLockReady $releaseJob $releaseFixture
  $releaseTimer = [Diagnostics.Stopwatch]::StartNew()
  Remove-SmokeFixture -FixtureRoot $releaseFixture.Root
  $releaseTimer.Stop()
  Assert-Condition (-not (Test-Path -LiteralPath $releaseFixture.Root)) 'Released profile fixture was not removed'
  Assert-Condition ($releaseTimer.Elapsed.TotalSeconds -ge 2) 'Cleanup did not wait for the held profile lock to release'
  Complete-SmokeLockJob $releaseJob $releaseFixture
  $releaseJob = $null
  "released lock: FileShare.None lock released after 3 seconds; cleanup succeeded after $([Math]::Round($releaseTimer.Elapsed.TotalSeconds, 1)) seconds"

  $persistentFixture = New-SmokeCleanupFixture
  $persistentJob = Start-SmokeLockJob $persistentFixture -1
  Wait-SmokeLockReady $persistentJob $persistentFixture
  $persistentTimer = [Diagnostics.Stopwatch]::StartNew()
  $timeoutError = $null
  try {
    Remove-SmokeFixture -FixtureRoot $persistentFixture.Root
  } catch {
    $timeoutError = $_
  }
  $persistentTimer.Stop()
  Assert-Condition ($timeoutError -and $timeoutError.Exception.Message -like '*after 30 seconds*') 'Persistent lock did not report the 30-second cleanup timeout'
  Assert-Condition ($timeoutError.Exception.Message -like "*retained at '$($persistentFixture.Root)'*") 'Timeout did not report the retained fixture path'
  Assert-Condition ($timeoutError.Exception.Message -match 'Windows error (32|33)') 'Timeout did not report a sharing or lock violation code'
  Assert-Condition ($timeoutError.Exception.Message -match "Last error id '.+', type 'System\..+', Windows error (32|33):") 'Timeout did not report the full last-error context'
  $windowsError = [regex]::Match($timeoutError.Exception.Message, 'Windows error (32|33)').Groups[1].Value
  Assert-Condition ($persistentTimer.Elapsed.TotalSeconds -ge 30 -and $persistentTimer.Elapsed.TotalSeconds -lt 45) "Persistent lock timed out at an unexpected duration: $($persistentTimer.Elapsed.TotalSeconds) seconds"
  Assert-Condition (Test-Path -LiteralPath $persistentFixture.Root) 'Persistent lock timeout removed the fixture root'
  Assert-Condition ([IO.File]::ReadAllText((Join-Path $persistentFixture.Root 'stdout.txt')) -eq 'preserved smoke stdout') 'Persistent lock timeout changed stdout'
  Assert-Condition ((Get-Item -LiteralPath (Join-Path $persistentFixture.Root 'stderr.txt')).Length -eq 0) 'Persistent lock timeout changed stderr'
  Assert-Condition (Test-Path -LiteralPath $persistentFixture.LockPath) 'Persistent lock timeout removed the held lockfile'
  "persistent lock: FileShare.None held until timeout; timed out after $([Math]::Round($persistentTimer.Elapsed.TotalSeconds, 1)) seconds on Windows error $windowsError; fixture, stdout, stderr, and lockfile retained"
  Complete-SmokeLockJob $persistentJob $persistentFixture
  $persistentJob = $null
  Remove-TestFixture $persistentFixture.Root
  $persistentFixture = $null

  $deniedFixture = New-SmokeCleanupFixture
  $script:smokeCleanupRemoveAttempts = 0
  function Invoke-SmokeCleanupDeniedRemoval {
    [CmdletBinding()]
    param([string]$LiteralPath, [switch]$Recurse, [switch]$Force)
    $script:smokeCleanupRemoveAttempts++
    throw [UnauthorizedAccessException]::new('synthetic non-sharing cleanup failure')
  }
  Set-Alias -Name Remove-Item -Value Invoke-SmokeCleanupDeniedRemoval -Scope Script -Force
  $deniedTimer = [Diagnostics.Stopwatch]::StartNew()
  $deniedError = $null
  try {
    Remove-SmokeFixture -FixtureRoot $deniedFixture.Root
  } catch {
    $deniedError = $_
  } finally {
    Microsoft.PowerShell.Utility\Remove-Alias -Name Remove-Item -Force
  }
  $deniedTimer.Stop()
  Assert-Condition ($deniedError -and $deniedError.Exception.Message -like '*synthetic non-sharing cleanup failure*') 'Non-sharing cleanup error was not rethrown'
  Assert-Condition ($script:smokeCleanupRemoveAttempts -eq 1) "Non-sharing cleanup error was retried $script:smokeCleanupRemoveAttempts times"
  Assert-Condition ($deniedTimer.Elapsed.TotalSeconds -lt 2) "Non-sharing cleanup error was not immediate: $($deniedTimer.Elapsed.TotalSeconds) seconds"
  Assert-Condition (Test-Path -LiteralPath $deniedFixture.Root) 'Non-sharing failure removed the fixture root'
  Assert-Condition ([IO.File]::ReadAllText((Join-Path $deniedFixture.Root 'stdout.txt')) -eq 'preserved smoke stdout') 'Non-sharing failure changed stdout'
  Assert-Condition ((Get-Item -LiteralPath (Join-Path $deniedFixture.Root 'stderr.txt')).Length -eq 0) 'Non-sharing failure changed stderr'
  "non-sharing error: rethrown after $script:smokeCleanupRemoveAttempts removal attempt; fixture and logs retained"
} finally {
  if ($releaseJob) { Complete-SmokeLockJob $releaseJob $releaseFixture }
  if ($persistentJob) { Complete-SmokeLockJob $persistentJob $persistentFixture }
  if ($persistentFixture) { Remove-TestFixture $persistentFixture.Root }
  if ($deniedFixture) { Remove-TestFixture $deniedFixture.Root }
}

#Requires -Version 5.1
<#
Tests package-windows-msi.ps1 with the native diskunfold.exe. Builds only into a new temporary directory
under the temp folder and removes only what it created. Nothing is installed.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string] $BinaryPath,
    [Parameter(Mandatory = $true)][string] $NoticesPath,
    [Parameter(Mandatory = $true)][string] $WixPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$packager = Join-Path $PSScriptRoot 'package-windows-msi.ps1'
$root = Join-Path ([System.IO.Path]::GetTempPath()) ('du-msi-test-' + [guid]::NewGuid().ToString('N'))
[System.IO.Directory]::CreateDirectory($root) | Out-Null

function Assert-True([bool] $Condition, [string] $Message) { if (-not $Condition) { throw $Message } }
function Assert-Fails([scriptblock] $Action, [string] $Pattern, [string] $Message) {
    $failed = $false
    try { & $Action | Out-Null } catch {
        if ($_.Exception.Message -notmatch $Pattern) { throw "$Message Got: $($_.Exception.Message)" }
        $failed = $true
    }
    if (-not $failed) { throw $Message }
}

try {
    $common = @{ BinaryPath = $BinaryPath; Version = '0.0.1'; Architecture = 'x64'; NoticesPath = $NoticesPath; WixPath = $WixPath }

    Assert-Fails { & $packager @common -OutputDirectory (Join-Path $root 'o1') -Version 0.1 } '' 'A version without three parts must be refused.'
    Assert-True (-not (Test-Path (Join-Path $root 'o1'))) 'A refused package must leave no output.'

    $real = Join-Path $root 'real-out'
    & $packager @common -OutputDirectory $real | Out-Null
    Assert-True (Test-Path (Join-Path $real 'DiskUnfold-0.0.1-x64.msi')) 'The MSI was not built.'
    Assert-Fails { & $packager @common -OutputDirectory $real } 'already exists' 'An existing output directory must be preserved.'
    $manifest = Get-Content -LiteralPath (Join-Path $real 'manifest.json') -Raw | ConvertFrom-Json
    $row = $manifest.rows[0]
    Assert-True ($row.format -ceq 'msi' -and $row.cpu -ceq 'x64' -and $row.signature -ceq 'unsigned') 'The manifest row has the wrong identity.'
    Assert-True ($row.sha256 -ceq (Get-FileHash -LiteralPath (Join-Path $real $row.file_name) -Algorithm SHA256).Hash.ToLowerInvariant()) 'The package digest is wrong.'
    Assert-True ($null -eq $row.PSObject.Properties['bundled_components']) 'The MSI row must not record bundled components.'
    foreach ($staged in $manifest.staged_files) {
        Assert-True ($staged.relative_path -notmatch '\\') 'Manifest paths must use forward slashes.'
        Assert-True (Test-Path (Join-Path $real $staged.relative_path)) "Staged file is missing: $($staged.relative_path)"
    }
    Assert-True ($row.executable_sha256 -ceq (Get-FileHash -LiteralPath $BinaryPath -Algorithm SHA256).Hash.ToLowerInvariant()) 'The executable digest is wrong.'
    Assert-True ((Get-Content -LiteralPath (Join-Path $real 'manifest.json') -Raw) -notmatch 'microsoft\.com|fwlink') 'The manifest must hold no Microsoft download link.'
    Write-Output 'MSI packager tests passed.'
}
finally {
    $resolved = [System.IO.Path]::GetFullPath($root)
    $temp = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    if ((Test-Path -LiteralPath $resolved) -and $resolved.StartsWith($temp, [System.StringComparison]::OrdinalIgnoreCase) -and (Split-Path -Leaf $resolved).StartsWith('du-msi-test-')) {
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}

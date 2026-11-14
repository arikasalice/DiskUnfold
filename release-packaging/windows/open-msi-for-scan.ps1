#Requires -Version 5.1
<#
.SYNOPSIS
Opens a built MSI into the tree layout that the artifact scan reads (release stage 4).

.DESCRIPTION
Extracts the MSI with an administrative install (msiexec /a), which writes the files that the package
installs. The tree holds diskunfold.exe, LICENSE, THIRD-PARTY-NOTICES.txt and install-form from the
extract. The MSI carries no other file; any extra member fails the script.
#>
[CmdletBinding()]
param(
    # Output directory of package-windows-msi.ps1 (holds manifest.json and the MSI).
    [Parameter(Mandatory = $true)]
    [string] $PackageDirectory,

    # Directory that receives <msi file name>/ ; created if missing.
    [Parameter(Mandatory = $true)]
    [string] $ScanRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'package-common.ps1')

$manifest = Get-Content -LiteralPath (Join-Path $PackageDirectory 'manifest.json') -Raw | ConvertFrom-Json
$row = @($manifest.rows)[0]
$msi = Get-RegularFile -Path (Join-Path $PackageDirectory $row.file_name) -Description 'MSI'
$members = @('diskunfold.exe', 'LICENSE', 'THIRD-PARTY-NOTICES.txt', 'install-form')

$work = Join-Path ([System.IO.Path]::GetTempPath()) ('du-msi-scan-' + [guid]::NewGuid().ToString('N'))
[System.IO.Directory]::CreateDirectory($work) | Out-Null
try {
    $extract = Join-Path $work 'extract'
    [System.IO.Directory]::CreateDirectory($extract) | Out-Null
    $process = Start-Process -FilePath 'msiexec.exe' -ArgumentList @('/a', ('"{0}"' -f $msi), '/qn', ('TARGETDIR="{0}"' -f $extract)) -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "administrative extract failed with exit code $($process.ExitCode)." }

    $tree = Join-Path ([System.IO.Path]::GetFullPath($ScanRoot)) $row.file_name
    if (Test-Path -LiteralPath $tree) { throw "Scan tree already exists: $tree" }
    [System.IO.Directory]::CreateDirectory($tree) | Out-Null
    foreach ($name in $members) {
        $found = @(Get-ChildItem -LiteralPath $extract -Recurse -File | Where-Object { $_.Name -ceq $name })
        if ($found.Count -ne 1) { throw "release.notice_mismatch: $($row.file_name) must contain exactly one $name; found $($found.Count)." }
        Copy-Item -LiteralPath $found[0].FullName -Destination (Join-Path $tree $name)
    }
    # Every file the administrative install wrote must be one of those four.
    $extra = @(Get-ChildItem -LiteralPath $extract -Recurse -File | Where-Object { $_.Name -cnotin $members -and $_.Extension -ne '.msi' })
    if ($extra.Count -gt 0) { throw "release.license_denied: unexpected members in $($row.file_name): $(($extra | ForEach-Object Name) -join ', ')" }
    Write-Output "Opened $($row.file_name) into $tree."
}
finally {
    $resolved = [System.IO.Path]::GetFullPath($work)
    if ($resolved.StartsWith([System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath()), [System.StringComparison]::OrdinalIgnoreCase) -and (Split-Path -Leaf $resolved).StartsWith('du-msi-scan-')) {
        Remove-Item -LiteralPath $resolved -Recurse -Force -ErrorAction SilentlyContinue
    }
}

#Requires -Version 5.1
<#
.SYNOPSIS
Opens a built MSI into the tree layout that the artifact scan reads (release stage 4).

.DESCRIPTION
Extracts the MSI with an administrative install (msiexec /a), which writes the files that the package
installs, and decompiles the Binary table with wix to get the embedded WebView2 installer. The tree
holds diskunfold.exe, LICENSE, THIRD-PARTY-NOTICES.txt, WEBVIEW2-NOTICE.txt and install-form from the
extract, plus WebView2-Installer.exe. The installer must carry a valid Authenticode signature from
CN=Microsoft Corporation and the SHA-256 that the packager recorded in the manifest; otherwise this
script fails and nothing is allowed through the scan. The scan then accepts that one binary by its
recorded digest and rejects every other binary in the tree.
#>
[CmdletBinding()]
param(
    # Output directory of package-windows-msi.ps1 (holds manifest.json and the MSI).
    [Parameter(Mandatory = $true)]
    [string] $PackageDirectory,

    # Directory that receives <msi file name>/ ; created if missing.
    [Parameter(Mandatory = $true)]
    [string] $ScanRoot,

    [Parameter(Mandatory = $true)]
    [string] $WixPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'package-common.ps1')

$manifest = Get-Content -LiteralPath (Join-Path $PackageDirectory 'manifest.json') -Raw | ConvertFrom-Json
$row = @($manifest.rows)[0]
$msi = Get-RegularFile -Path (Join-Path $PackageDirectory $row.file_name) -Description 'MSI'
$component = @($row.bundled_components)[0]

$work = Join-Path ([System.IO.Path]::GetTempPath()) ('du-msi-scan-' + [guid]::NewGuid().ToString('N'))
[System.IO.Directory]::CreateDirectory($work) | Out-Null
try {
    $extract = Join-Path $work 'extract'
    [System.IO.Directory]::CreateDirectory($extract) | Out-Null
    $process = Start-Process -FilePath 'msiexec.exe' -ArgumentList @('/a', ('"{0}"' -f $msi), '/qn', ('TARGETDIR="{0}"' -f $extract)) -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "administrative extract failed with exit code $($process.ExitCode)." }

    $decompiled = Join-Path $work 'decompile'
    $output = @(& $WixPath msi decompile -x $decompiled -o (Join-Path $work 'decompiled.wxs') $msi 2>&1)
    if ($LASTEXITCODE -ne 0) { throw "wix msi decompile failed: $($output -join [Environment]::NewLine)" }
    $installer = Get-RegularFile -Path (Join-Path $decompiled 'Binary\WebView2Installer') -Description 'embedded WebView2 installer'

    $signature = Get-AuthenticodeSignature -FilePath $installer
    if ($signature.Status -ne [System.Management.Automation.SignatureStatus]::Valid) {
        throw "release.signature_unverified: the WebView2 installer in $($row.file_name) has Authenticode status $($signature.Status)."
    }
    if (-not (@($signature.SignerCertificate.Subject -split ', ') -contains 'CN=Microsoft Corporation')) {
        throw "release.signature_unverified: the WebView2 installer in $($row.file_name) is signed by $($signature.SignerCertificate.Subject)."
    }
    $digest = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($digest -ne $component.sha256) {
        throw "release.notice_mismatch: the WebView2 installer in $($row.file_name) has SHA-256 $digest, not the recorded $($component.sha256)."
    }

    $tree = Join-Path ([System.IO.Path]::GetFullPath($ScanRoot)) $row.file_name
    if (Test-Path -LiteralPath $tree) { throw "Scan tree already exists: $tree" }
    [System.IO.Directory]::CreateDirectory($tree) | Out-Null
    foreach ($name in 'diskunfold.exe', 'LICENSE', 'THIRD-PARTY-NOTICES.txt', 'WEBVIEW2-NOTICE.txt', 'install-form') {
        $found = @(Get-ChildItem -LiteralPath $extract -Recurse -File | Where-Object { $_.Name -ceq $name })
        if ($found.Count -ne 1) { throw "release.notice_mismatch: $($row.file_name) must contain exactly one $name; found $($found.Count)." }
        Copy-Item -LiteralPath $found[0].FullName -Destination (Join-Path $tree $name)
    }
    # Every file the administrative install wrote must be one of those five.
    $extra = @(Get-ChildItem -LiteralPath $extract -Recurse -File | Where-Object { $_.Name -cnotin @('diskunfold.exe', 'LICENSE', 'THIRD-PARTY-NOTICES.txt', 'WEBVIEW2-NOTICE.txt', 'install-form') -and $_.Extension -ne '.msi' })
    if ($extra.Count -gt 0) { throw "release.license_denied: unexpected members in $($row.file_name): $(($extra | ForEach-Object Name) -join ', ')" }
    Copy-Item -LiteralPath $installer -Destination (Join-Path $tree 'WebView2-Installer.exe')
    Write-Output "Opened $($row.file_name) into $tree (WebView2 installer $($component.version), $digest, signer verified)."
}
finally {
    $resolved = [System.IO.Path]::GetFullPath($work)
    if ($resolved.StartsWith([System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath()), [System.StringComparison]::OrdinalIgnoreCase) -and (Split-Path -Leaf $resolved).StartsWith('du-msi-scan-')) {
        Remove-Item -LiteralPath $resolved -Recurse -Force -ErrorAction SilentlyContinue
    }
}
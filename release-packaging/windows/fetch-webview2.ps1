#Requires -Version 5.1
<#
.SYNOPSIS
Downloads the WebView2 Evergreen Standalone Installer from Microsoft's permanent link and verifies it.

.DESCRIPTION
The installer is a build input and is never committed. It is fetched from Microsoft's fixed link for the
architecture, must carry a valid Authenticode signature from CN=Microsoft Corporation, and must be a PE
file for the requested architecture. Microsoft replaces the file in place, so no SHA-256 is pinned: the
digest and version are printed so the packager and the release inventory record what was shipped.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('x64', 'arm64')]
    [string] $Architecture,

    # New file to write. It must not exist.
    [Parameter(Mandatory = $true)]
    [string] $OutputPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'package-common.ps1')

$links = @{
    x64 = 'https://go.microsoft.com/fwlink/?linkid=2124701'
    arm64 = 'https://go.microsoft.com/fwlink/?linkid=2099616'
}
$target = [System.IO.Path]::GetFullPath($OutputPath)
if (Test-Path -LiteralPath $target) { throw "Output file already exists: $target" }
$parent = [System.IO.Directory]::GetParent($target).FullName
if (-not (Test-Path -LiteralPath $parent -PathType Container)) { throw "Output directory does not exist: $parent" }

$partial = '{0}.partial' -f $target
$ProgressPreference = 'SilentlyContinue'
$downloaded = $false
for ($attempt = 1; $attempt -le 3 -and -not $downloaded; $attempt++) {
    try {
        Invoke-WebRequest -Uri $links[$Architecture] -OutFile $partial -UseBasicParsing -MaximumRedirection 5
        $downloaded = $true
    }
    catch {
        if (Test-Path -LiteralPath $partial) { Remove-Item -LiteralPath $partial -Force }
        if ($attempt -eq 3) { throw }
        Start-Sleep -Seconds (5 * $attempt)
    }
}

try {
    $signature = Get-AuthenticodeSignature -FilePath $partial
    if ($signature.Status -ne [System.Management.Automation.SignatureStatus]::Valid) {
        throw "The downloaded installer has no valid Authenticode signature; status is $($signature.Status)."
    }
    $subject = $signature.SignerCertificate.Subject
    if (-not (@($subject -split ', ') -contains 'CN=Microsoft Corporation')) {
        throw "The downloaded installer is signed by $subject, not CN=Microsoft Corporation."
    }
    Move-Item -LiteralPath $partial -Destination $target
}
catch {
    if (Test-Path -LiteralPath $partial) { Remove-Item -LiteralPath $partial -Force }
    throw
}
$info = Get-Item -LiteralPath $target
Write-Output ('Downloaded {0} ({1} bytes)' -f $info.Name, $info.Length)
Write-Output ('Version: {0}' -f $info.VersionInfo.ProductVersion)
Write-Output ('SHA-256: {0}' -f (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash.ToLowerInvariant())
Write-Output ('Signer: {0}' -f $subject)

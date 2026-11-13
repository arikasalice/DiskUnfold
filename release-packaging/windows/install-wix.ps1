#Requires -Version 5.1
<#
.SYNOPSIS
Installs the pinned WiX Toolset as an isolated .NET tool.

.DESCRIPTION
Downloads the wix NuGet package named in wix-tool.json, refuses it unless its SHA-256 matches the pin,
and installs it into -ToolPath with its own NuGet package folder, so nothing is written to the user's
global NuGet cache. The package needs the .NET SDK (dotnet). The script uses no WiX extension, so WiX
never writes its extension cache. Prints the path of wix.exe.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $ToolPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$pin = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'wix-tool.json') -Raw | ConvertFrom-Json
$toolFull = [System.IO.Path]::GetFullPath($ToolPath)
$source = Join-Path $toolFull 'source'
$packages = Join-Path $toolFull 'nuget-packages'
$install = Join-Path $toolFull 'wix'
foreach ($directory in $toolFull, $source, $packages) {
    [System.IO.Directory]::CreateDirectory($directory) | Out-Null
}

$package = Join-Path $source ('wix.{0}.nupkg' -f $pin.version)
Invoke-WebRequest -Uri $pin.nupkg_url -OutFile $package -UseBasicParsing
$actual = (Get-FileHash -LiteralPath $package -Algorithm SHA256).Hash.ToLowerInvariant()
if ($actual -ne $pin.nupkg_sha256) {
    Remove-Item -LiteralPath $package -Force
    throw "The wix package SHA-256 is $actual, not the pinned $($pin.nupkg_sha256)."
}

$env:NUGET_PACKAGES = $packages
$env:DOTNET_CLI_TELEMETRY_OPTOUT = '1'
$env:DOTNET_NOLOGO = '1'
$env:DOTNET_SKIP_FIRST_TIME_EXPERIENCE = '1'
& dotnet tool install wix --version $pin.version --tool-path $install --add-source $source --ignore-failed-sources | Out-Host
if ($LASTEXITCODE -ne 0) { throw "dotnet tool install failed with exit code $LASTEXITCODE." }

$wix = Join-Path $install 'wix.exe'
$reported = (& $wix --version).Trim()
if (-not $reported.StartsWith($pin.version + '+')) { throw "wix.exe reports $reported, not $($pin.version)." }
Write-Output $wix
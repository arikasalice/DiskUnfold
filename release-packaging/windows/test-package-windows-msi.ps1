#Requires -Version 5.1
<#
Tests package-windows-msi.ps1 with the native diskunfold.exe. Builds only into a new temporary directory
under the temp folder and removes only what it created. Nothing is installed. When -WebView2InstallerPath
(a Microsoft-signed installer) is given, it also builds the real package and checks the manifest row.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string] $BinaryPath,
    [Parameter(Mandatory = $true)][string] $NoticesPath,
    [Parameter(Mandatory = $true)][string] $WixPath,
    [string] $WebView2InstallerPath
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
    # An unsigned stand-in named like Microsoft's file: the signature gate must refuse it.
    $stubDir = Join-Path $root 'stub'
    [System.IO.Directory]::CreateDirectory($stubDir) | Out-Null
    $stub = Join-Path $stubDir 'MicrosoftEdgeWebView2RuntimeInstallerX64.exe'
    Copy-Item -LiteralPath $BinaryPath -Destination $stub
    # Different bytes from the executable, so the embedded-installer comparison finds one match.
    [System.IO.File]::AppendAllText($stub, 'stub')
    $common = @{ BinaryPath = $BinaryPath; Version = '0.1.0'; Architecture = 'x64'; NoticesPath = $NoticesPath; WixPath = $WixPath }

    Assert-Fails { & $packager @common -OutputDirectory (Join-Path $root 'o1') -WebView2InstallerPath $stub } 'valid Authenticode signature' 'An unsigned WebView2 installer must be refused.'
    Assert-True (-not (Test-Path (Join-Path $root 'o1'))) 'A refused package must leave no output.'
    Assert-Fails { & $packager @common -OutputDirectory (Join-Path $root 'o2') -WebView2InstallerPath $stub -TestOnlyWebView2ClientKey 'Software\X' } 'only allowed together' 'The test-only key must require the skip switch.'
    Assert-Fails { & $packager @common -OutputDirectory (Join-Path $root 'o3') -WebView2InstallerPath $stub -TestOnlySkipWebView2Signature } 'requires TestOnlyWebView2ClientKey' 'The skip switch must require the test key.'

    $noWebView2Notice = Join-Path $root 'notices-without-section.txt'
    Set-Content -LiteralPath $noWebView2Notice -Value 'notices'
    $noSection = @{ BinaryPath = $BinaryPath; Version = '0.1.0'; Architecture = 'x64'; NoticesPath = $noWebView2Notice; WixPath = $WixPath; OutputDirectory = (Join-Path $root 'o4'); WebView2InstallerPath = $stub }
    Assert-Fails { & $packager @noSection } 'no WebView2 section' 'Notices without the WebView2 section must be refused.'
    $armStub = Join-Path $stubDir 'MicrosoftEdgeWebView2RuntimeInstallerARM64.exe'
    Copy-Item -LiteralPath $BinaryPath -Destination $armStub
    Assert-Fails { & $packager @common -OutputDirectory (Join-Path $root 'o5') -WebView2InstallerPath $armStub } 'must be named' 'An installer of the other architecture must be refused.'
    Assert-Fails { & $packager @common -OutputDirectory (Join-Path $root 'o6') -WebView2InstallerPath $stub -Version 0.1 } '' 'A version without three parts must be refused.'

    # A package built with the stub opens with an administrative extract and keeps the five members.
    $stubOut = Join-Path $root 'stub-out'
    & $packager @common -OutputDirectory $stubOut -WebView2InstallerPath $stub -TestOnlyWebView2ClientKey 'Software\DiskUnfold\Test\NoWebView2' -TestOnlySkipWebView2Signature 3>$null | Out-Null
    Assert-True (Test-Path (Join-Path $stubOut 'DiskUnfold-0.1.0-x64.msi')) 'The stub MSI was not built.'
    Assert-True (-not (Test-Path (Join-Path $stubOut 'manifest.json'))) 'A test-only package must not write a manifest.'
    Assert-Fails { & $packager @common -OutputDirectory $stubOut -WebView2InstallerPath $stub -TestOnlyWebView2ClientKey 'Software\DiskUnfold\Test\NoWebView2' -TestOnlySkipWebView2Signature } 'already exists' 'An existing output directory must be preserved.'

    if ($WebView2InstallerPath) {
        $real = Join-Path $root 'real-out'
        & $packager @common -OutputDirectory $real -WebView2InstallerPath $WebView2InstallerPath | Out-Null
        $manifest = Get-Content -LiteralPath (Join-Path $real 'manifest.json') -Raw | ConvertFrom-Json
        $row = $manifest.rows[0]
        Assert-True ($row.format -ceq 'msi' -and $row.cpu -ceq 'x64' -and $row.signature -ceq 'unsigned') 'The manifest row has the wrong identity.'
        Assert-True ($row.sha256 -ceq (Get-FileHash -LiteralPath (Join-Path $real $row.file_name) -Algorithm SHA256).Hash.ToLowerInvariant()) 'The package digest is wrong.'
        $component = $row.bundled_components[0]
        Assert-True ($component.sha256 -ceq (Get-FileHash -LiteralPath $WebView2InstallerPath -Algorithm SHA256).Hash.ToLowerInvariant()) 'The WebView2 digest is wrong.'
        Assert-True ($component.signer -match 'CN=Microsoft Corporation') 'The WebView2 signer is not recorded.'
        Assert-True ($component.version -match '^\d+(\.\d+){3}$') 'The WebView2 version is not recorded.'
        foreach ($staged in $manifest.staged_files) {
            Assert-True ($staged.relative_path -notmatch '\\') 'Manifest paths must use forward slashes.'
            Assert-True (Test-Path (Join-Path $real $staged.relative_path)) "Staged file is missing: $($staged.relative_path)"
        }
        Assert-True ($row.executable_sha256 -ceq (Get-FileHash -LiteralPath $BinaryPath -Algorithm SHA256).Hash.ToLowerInvariant()) 'The executable digest is wrong.'
    }
    Write-Output 'MSI packager tests passed.'
}
finally {
    $resolved = [System.IO.Path]::GetFullPath($root)
    $temp = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    if ((Test-Path -LiteralPath $resolved) -and $resolved.StartsWith($temp, [System.StringComparison]::OrdinalIgnoreCase) -and (Split-Path -Leaf $resolved).StartsWith('du-msi-test-')) {
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
